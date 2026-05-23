#![warn(clippy::undocumented_unsafe_blocks)]

use std::collections::VecDeque;
mod native;
mod system;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use player_backend_ffmpeg::{
    CompressedVideoPacket, FfmpegBackend, VideoDecodeInfo as BackendVideoDecodeInfo,
    VideoDecoderMode as BackendVideoDecoderMode, VideoPacketSource, VideoPacketStreamInfo,
};
use player_model::{MediaSource, MediaSourceProtocol};
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
    DecoderBitstreamFormat, DecoderMediaKind, DecoderNativeFrame, DecoderNativeHandleKind,
    DecoderPacket, DecoderReceiveNativeFrameOutput, DecoderSessionConfig, FrameProcessorError,
    FrameProcessorOutputFrame, FrameProcessorReceiveOutput, FrameProcessorSession,
    FrameProcessorSessionConfig, FrameProcessorSubmitFrame, FrameProcessorSubmitResult,
    FrameProcessorSubmitStatus, NativeDecoderSession, NativeFrame, NativeFrameMetadata,
    NativeHandleKind, SourceNormalizerPacketMediaKind, SourceNormalizerPacketSeek,
    SourceNormalizerPacketSession, SourceNormalizerPacketSessionConfig,
    SourceNormalizerPacketTrackInfo, SourceNormalizerReadPacketMetadata,
    SourceNormalizerReadPacketStatus, VesperPluginKind,
};
use player_plugin_loader::{
    DecoderPluginCapabilitySummary, DecoderPluginCodecSummary, DecoderPluginMatchRequest,
    FrameProcessorPluginCapabilitySummary, LoadedDynamicPlugin, PluginCapabilitySummary,
    PluginDiagnosticRecord, PluginDiagnosticStatus, PluginRegistry,
};
use player_runtime::{
    DecodedVideoFrame, FrameProcessorMode, FrameProcessorPolicy, FrameProcessorPolicyAction,
    FrameProcessorWarning, FrameProcessorWarningKind, PlaybackProgress,
    PlayerDecoderPluginVideoMode, PlayerError, PlayerErrorCode, PlayerFrameProcessingMetrics,
    PlayerMediaInfo, PlayerPluginCapabilitySummary, PlayerPluginCodecCapability,
    PlayerPluginDecoderCapabilitySummary, PlayerPluginDiagnostic, PlayerPluginDiagnosticStatus,
    PlayerPluginFrameProcessorCapabilitySummary, PlayerResult, PlayerRuntime, PlayerRuntimeAdapter,
    PlayerRuntimeAdapterBackendFamily, PlayerRuntimeAdapterBootstrap,
    PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory, PlayerRuntimeAdapterInitializer,
    PlayerRuntimeBootstrap, PlayerRuntimeCommand, PlayerRuntimeCommandResult, PlayerRuntimeEvent,
    PlayerRuntimeInitializer, PlayerRuntimeOptions, PlayerRuntimeStartup, PlayerRuntimeWarning,
    PlayerVideoDecodeInfo, PlayerVideoDecodeMode, PlayerVideoSurfaceTarget, PresentationState,
    SourceNormalizerMode, register_default_runtime_adapter_factory,
};
use tracing::info;

pub const MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID: &str = "macos_software_desktop";
pub const MACOS_HOST_PLAYER_RUNTIME_ADAPTER_ID: &str = "macos_host";
const MACOS_NATIVE_FRAME_PREFETCH_COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(50);
const MACOS_NATIVE_FRAME_DECODER_DRAIN_RETRY_INTERVAL: Duration = Duration::from_millis(1);
const FRAME_PROCESSOR_DEBUG_ENV: &str = "VESPER_FRAME_PROCESSOR_DEBUG";
const FRAME_PROCESSOR_DEBUG_TRACE_ENV: &str = "VESPER_FRAME_PROCESSOR_DEBUG_TRACE";
const FRAME_PROCESSOR_DEBUG_WINDOW_ENV: &str = "VESPER_FRAME_PROCESSOR_DEBUG_WINDOW";
const DEFAULT_FRAME_PROCESSOR_DEBUG_WINDOW: u64 = 120;
const SOURCE_NORMALIZER_STARTUP_TIMEOUT: Duration = Duration::from_millis(5_000);
const SOURCE_NORMALIZER_SESSION_TIMEOUT: Duration = Duration::from_millis(40_000);

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

pub fn install_default_macos_runtime_adapter_factory() -> PlayerResult<()> {
    install_default_macos_host_runtime_adapter_factory()
}

pub fn install_default_macos_host_runtime_adapter_factory() -> PlayerResult<()> {
    register_default_runtime_adapter_factory(macos_host_runtime_adapter_factory())
}

pub fn install_default_macos_software_runtime_adapter_factory() -> PlayerResult<()> {
    register_default_runtime_adapter_factory(macos_runtime_adapter_factory())
}

pub fn install_default_macos_native_runtime_adapter_factory() -> PlayerResult<()> {
    register_default_runtime_adapter_factory(macos_native_runtime_adapter_factory())
}

pub fn open_macos_host_runtime_uri_with_options(
    uri: impl Into<String>,
    options: PlayerRuntimeOptions,
) -> PlayerResult<PlayerRuntimeBootstrap> {
    open_macos_host_runtime_source_with_options(MediaSource::new(uri), options)
}

pub fn open_macos_host_runtime_uri_with_options_and_interrupt(
    uri: impl Into<String>,
    options: PlayerRuntimeOptions,
    interrupt_flag: Arc<AtomicBool>,
) -> PlayerResult<PlayerRuntimeBootstrap> {
    open_macos_host_runtime_source_with_options_and_interrupt(
        MediaSource::new(uri),
        options,
        interrupt_flag,
    )
}

pub fn open_macos_software_runtime_uri_with_options_and_interrupt(
    uri: impl Into<String>,
    options: PlayerRuntimeOptions,
    interrupt_flag: Arc<AtomicBool>,
) -> PlayerResult<PlayerRuntimeBootstrap> {
    open_macos_software_runtime_source_with_options_and_interrupt(
        MediaSource::new(uri),
        options,
        interrupt_flag,
    )
}

pub fn probe_macos_host_runtime_uri_with_options(
    uri: impl Into<String>,
    options: PlayerRuntimeOptions,
) -> PlayerResult<MacosHostRuntimeProbe> {
    probe_macos_host_runtime_source_with_options(MediaSource::new(uri), options)
}

pub fn probe_macos_host_runtime_source_with_options(
    source: MediaSource,
    options: PlayerRuntimeOptions,
) -> PlayerResult<MacosHostRuntimeProbe> {
    if !cfg!(target_os = "macos") {
        return Err(PlayerError::new(
            PlayerErrorCode::Unsupported,
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
) -> PlayerResult<PlayerRuntimeBootstrap> {
    if !cfg!(target_os = "macos") {
        return Err(PlayerError::new(
            PlayerErrorCode::Unsupported,
            "macos host runtime strategy can only be initialized on macOS targets",
        ));
    }

    let normalization = prepare_source_normalizer_for_open(source, &options)?;
    let source = normalization.source.clone();
    if normalization.has_packet_stream() {
        return open_macos_software_runtime_with_prepared_normalization(
            source,
            options,
            Arc::new(AtomicBool::new(false)),
            normalization,
            Some(
                "source normalizer packet stream selected; routed to desktop decoder plugin path"
                    .to_owned(),
            ),
        );
    }

    let native_factory = macos_system_native_runtime_adapter_factory();

    let native_initializer = PlayerRuntimeInitializer::probe_source_with_factory(
        source.clone(),
        options.clone(),
        native_factory,
    );

    match native_initializer {
        Ok(initializer)
            if should_prefer_native_host_runtime(&initializer.media_info(), &options) =>
        {
            let media_info = initializer.media_info();
            match initializer.initialize() {
                Ok(mut bootstrap) => {
                    bootstrap.startup =
                        apply_decoder_plugin_diagnostics(bootstrap.startup, &media_info, &options);
                    bootstrap.startup =
                        apply_source_normalizer_open_diagnostics(bootstrap.startup, &normalization);
                    Ok(attach_source_normalizer_to_runtime(
                        bootstrap,
                        normalization,
                    ))
                }
                Err(native_error) => open_software_fallback_runtime(
                    source,
                    options,
                    Some(format!(
                        "macos native host runtime failed to initialize; falling back to software desktop path: {}",
                        native_error.message()
                    )),
                    normalization,
                ),
            }
        }
        Ok(initializer) => {
            let fallback_reason =
                macos_host_software_path_reason(&initializer.media_info(), &options);
            open_software_fallback_runtime(source, options, fallback_reason, normalization)
        }
        Err(native_error) => open_software_fallback_runtime(
            source,
            options,
            Some(format!(
                "macos native host runtime probe failed; selected software desktop path: {}",
                native_error.message()
            )),
            normalization,
        ),
    }
}

pub fn open_macos_host_runtime_source_with_options_and_interrupt(
    source: MediaSource,
    options: PlayerRuntimeOptions,
    interrupt_flag: Arc<AtomicBool>,
) -> PlayerResult<PlayerRuntimeBootstrap> {
    if !cfg!(target_os = "macos") {
        return Err(PlayerError::new(
            PlayerErrorCode::Unsupported,
            "macos host runtime strategy can only be initialized on macOS targets",
        ));
    }

    let normalization = prepare_source_normalizer_for_open(source, &options)?;
    let source = normalization.source.clone();
    if normalization.has_packet_stream() {
        return open_macos_software_runtime_with_prepared_normalization(
            source,
            options,
            interrupt_flag,
            normalization,
            Some(
                "source normalizer packet stream selected; routed to desktop decoder plugin path"
                    .to_owned(),
            ),
        );
    }

    let native_factory = macos_system_native_runtime_adapter_factory();

    let native_initializer = PlayerRuntimeInitializer::probe_source_with_factory(
        source.clone(),
        options.clone(),
        native_factory,
    );

    match native_initializer {
        Ok(initializer)
            if should_prefer_native_host_runtime(&initializer.media_info(), &options) =>
        {
            let media_info = initializer.media_info();
            match initializer.initialize() {
                Ok(mut bootstrap) => {
                    bootstrap.startup =
                        apply_decoder_plugin_diagnostics(bootstrap.startup, &media_info, &options);
                    bootstrap.startup =
                        apply_source_normalizer_open_diagnostics(bootstrap.startup, &normalization);
                    Ok(attach_source_normalizer_to_runtime(
                        bootstrap,
                        normalization,
                    ))
                }
                Err(native_error) => open_software_fallback_runtime_with_interrupt(
                    source,
                    options,
                    interrupt_flag,
                    Some(format!(
                        "macos native host runtime failed to initialize; falling back to software desktop path: {}",
                        native_error.message()
                    )),
                    normalization,
                ),
            }
        }
        Ok(initializer) => {
            let fallback_reason =
                macos_host_software_path_reason(&initializer.media_info(), &options);
            open_software_fallback_runtime_with_interrupt(
                source,
                options,
                interrupt_flag,
                fallback_reason,
                normalization,
            )
        }
        Err(native_error) => open_software_fallback_runtime_with_interrupt(
            source,
            options,
            interrupt_flag,
            Some(format!(
                "macos native host runtime probe failed; selected software desktop path: {}",
                native_error.message()
            )),
            normalization,
        ),
    }
}

pub fn open_macos_software_runtime_source_with_options_and_interrupt(
    source: MediaSource,
    options: PlayerRuntimeOptions,
    interrupt_flag: Arc<AtomicBool>,
) -> PlayerResult<PlayerRuntimeBootstrap> {
    let normalization = prepare_source_normalizer_for_open(source, &options)?;
    open_macos_software_runtime_with_prepared_normalization(
        normalization.source.clone(),
        options,
        interrupt_flag,
        normalization,
        None,
    )
}

fn open_macos_software_runtime_with_prepared_normalization(
    source: MediaSource,
    options: PlayerRuntimeOptions,
    interrupt_flag: Arc<AtomicBool>,
    mut normalization: MacosSourceNormalizationOutcome,
    fallback_reason: Option<String>,
) -> PlayerResult<PlayerRuntimeBootstrap> {
    let source_normalizer_packet_session = normalization.packet_session.clone();
    let packet_selection = select_macos_source_normalizer_packet_decoder(
        normalization.packet_stream_info.as_ref(),
        &options,
    );
    let selection = if packet_selection.is_some() {
        packet_selection
    } else {
        probe_platform_desktop_source_with_options(
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
        })
    };
    let selected_plugin_name = selection
        .as_ref()
        .and_then(|selection| selection.plugin_name.clone());

    let open_result = match selection.clone() {
        Some(selection) if normalization.has_packet_stream() => {
            let packet_session = normalization.packet_session.clone().ok_or_else(|| {
                PlayerError::new(
                    PlayerErrorCode::BackendFailure,
                    "source normalizer packet stream was selected without an open packet session",
                )
            })?;
            open_platform_desktop_source_with_video_source_factory_and_options_and_interrupt(
                MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                source.clone(),
                options.clone(),
                interrupt_flag.clone(),
                Arc::new(MacosSourceNormalizerPacketVideoSourceFactory {
                    decoder_plugin_path: selection.plugin_path,
                    decoder_plugin_name: selection.plugin_name,
                    video_surface: selection.video_surface,
                    frame_processor_paths: selection.frame_processor_paths,
                    frame_processor_mode: selection.frame_processor_mode,
                    frame_processor_policy: selection.frame_processor_policy,
                    packet_session,
                }),
                macos_native_frame_decoder_capabilities(),
            )
        }
        Some(selection) => {
            open_platform_desktop_source_with_video_source_factory_and_options_and_interrupt(
                MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                source.clone(),
                options.clone(),
                interrupt_flag.clone(),
                Arc::new(MacosNativeFrameVideoSourceFactory {
                    plugin_path: selection.plugin_path,
                    video_surface: selection.video_surface,
                    frame_processor_paths: selection.frame_processor_paths,
                    frame_processor_mode: selection.frame_processor_mode,
                    frame_processor_policy: selection.frame_processor_policy,
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
        mut startup,
    } = match (open_result, selection) {
        (Ok(bootstrap), _) => bootstrap,
        (Err(native_error), Some(selection)) if strict_frame_processor_selection(&selection) => {
            return Err(PlayerError::new(
                PlayerErrorCode::BackendFailure,
                format!(
                    "native-frame frame processor initialization failed in strict mode: {}",
                    native_error.message()
                ),
            ));
        }
        (Err(native_error), Some(_)) if normalization.has_packet_stream() => {
            let message = format!(
                "source normalizer packet stream decoder plugin initialization failed: {}",
                native_error.message()
            );
            if options.source_normalizer_mode == SourceNormalizerMode::RequireNormalized {
                return Err(PlayerError::new(PlayerErrorCode::BackendFailure, message));
            }
            normalization
                .diagnostics
                .push(source_normalizer_runtime_diagnostic(None, message));
            drop_source_normalizer_packet_session(&mut normalization);
            let mut bootstrap = open_platform_desktop_source_with_options_and_interrupt(
                MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                source.clone(),
                options.clone(),
                interrupt_flag,
            )?;
            apply_video_decode_fallback_reason(
                &mut bootstrap.startup,
                Some(format!(
                    "source normalizer packet stream decoder plugin initialization failed; selected FFmpeg software path: {}",
                    native_error.message()
                )),
            );
            bootstrap
        }
        (Err(native_error), None) if normalization.has_packet_stream() => {
            let message = source_normalizer_packet_decoder_unavailable_message(
                &normalization,
                &options,
            )
            .unwrap_or_else(|| {
                format!(
                    "source normalizer packet stream did not find a matching decoder plugin: {}",
                    native_error.message()
                )
            });
            if options.source_normalizer_mode == SourceNormalizerMode::RequireNormalized {
                return Err(PlayerError::new(PlayerErrorCode::Unsupported, message));
            }
            normalization
                .diagnostics
                .push(source_normalizer_runtime_diagnostic(None, message));
            drop_source_normalizer_packet_session(&mut normalization);
            let mut bootstrap = open_platform_desktop_source_with_options_and_interrupt(
                MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                source.clone(),
                options.clone(),
                interrupt_flag,
            )?;
            apply_video_decode_fallback_reason(
                &mut bootstrap.startup,
                Some(format!(
                    "source normalizer packet stream did not find a matching decoder plugin; selected FFmpeg software path: {}",
                    native_error.message()
                )),
            );
            bootstrap
        }
        (Err(native_error), Some(_)) => {
            let mut bootstrap = open_platform_desktop_source_with_options_and_interrupt(
                MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
                source.clone(),
                options.clone(),
                interrupt_flag,
            )?;
            apply_video_decode_fallback_reason(
                &mut bootstrap.startup,
                Some(format!(
                    "native-frame decoder plugin initialization failed; selected FFmpeg software path: {}",
                    native_error.message()
                )),
            );
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
    apply_video_decode_fallback_reason(&mut startup, fallback_reason);
    let runtime_fallback = (diagnostics.has_video_surface && !normalization.has_packet_stream())
        .then(|| MacosRuntimeActiveFallback {
            source,
            options: options.clone(),
            fallback_reason:
                "native-frame runtime failed during playback; selected FFmpeg software path"
                    .to_owned(),
        });

    Ok(PlayerRuntime::from_adapter_bootstrap(
        MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
        PlayerRuntimeAdapterBootstrap {
            runtime: Box::new(MacosRuntimeAdapter {
                inner: runtime,
                video_decode: diagnostics.video_decode.clone(),
                plugin_diagnostics: diagnostics.plugin_diagnostics.clone(),
                has_video_surface: diagnostics.has_video_surface,
                runtime_fallback,
                pending_runtime_fallback_events: VecDeque::new(),
                source_normalizer_packet_session,
            }),
            initial_frame,
            startup: apply_source_normalizer_open_diagnostics(
                apply_macos_runtime_diagnostics(startup, &diagnostics),
                &normalization,
            ),
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
    ) -> PlayerResult<Box<dyn PlayerRuntimeAdapterInitializer>>;
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
    strict_frame_processor_error_prefix: Option<String>,
}

struct MacosRuntimeAdapterFallback {
    inner: Box<dyn PlayerRuntimeAdapterInitializer>,
    diagnostics: MacosRuntimeDiagnostics,
    fallback_reason: String,
}

struct MacosSourceNormalizationOutcome {
    source: MediaSource,
    packet_session: Option<Arc<Mutex<Option<Box<dyn SourceNormalizerPacketSession>>>>>,
    packet_stream_info: Option<player_plugin::SourceNormalizerPacketStreamInfo>,
    diagnostics: Vec<PlayerPluginDiagnostic>,
    selected_profile: Option<String>,
    normalized_endpoint: Option<String>,
    ready_latency: Option<Duration>,
}

impl MacosSourceNormalizationOutcome {
    fn has_packet_stream(&self) -> bool {
        self.packet_session.is_some() && self.packet_stream_info.is_some()
    }
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
    plugin_diagnostics: Vec<PlayerPluginDiagnostic>,
    has_video_surface: bool,
    runtime_fallback: Option<MacosRuntimeActiveFallback>,
    pending_runtime_fallback_events: VecDeque<PlayerRuntimeEvent>,
    #[allow(dead_code)]
    source_normalizer_packet_session:
        Option<Arc<Mutex<Option<Box<dyn SourceNormalizerPacketSession>>>>>,
}

impl PlayerRuntimeAdapterFactory for MacosHostPlayerRuntimeAdapterFactory {
    fn adapter_id(&self) -> &'static str {
        MACOS_HOST_PLAYER_RUNTIME_ADAPTER_ID
    }

    fn probe_source_with_options(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
    ) -> PlayerResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
        if !cfg!(target_os = "macos") {
            return Err(PlayerError::new(
                PlayerErrorCode::Unsupported,
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
    ) -> PlayerResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
        if !cfg!(target_os = "macos") {
            return Err(PlayerError::new(
                PlayerErrorCode::Unsupported,
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
                    frame_processor_paths: selection.frame_processor_paths.clone(),
                    frame_processor_mode: selection.frame_processor_mode,
                    frame_processor_policy: selection.frame_processor_policy.clone(),
                }),
                capabilities,
            )?;
            let media_info = native_inner.media_info();
            let mut diagnostics = macos_runtime_diagnostics(&media_info, &options);
            diagnostics.video_decode =
                macos_native_frame_decoder_video_decode_info(selection.plugin_name.as_deref());
            diagnostics.has_video_surface = true;

            let strict_frame_processor = strict_frame_processor_selection(&selection);
            let (fallback, runtime_fallback, strict_frame_processor_error_prefix) =
                if strict_frame_processor {
                    let strict_error_prefix =
                        "native-frame frame processor initialization failed in strict mode"
                            .to_owned();
                    (None, None, Some(strict_error_prefix))
                } else {
                    let fallback = MacosRuntimeAdapterFallback {
                        inner,
                        diagnostics: fallback_diagnostics,
                        fallback_reason: "native-frame decoder plugin initialization failed; selected FFmpeg software path"
                            .to_owned(),
                    };
                    let runtime_fallback = MacosRuntimeActiveFallback {
                        source: source.clone(),
                        options: options.clone(),
                        fallback_reason:
                            "native-frame runtime failed during playback; selected FFmpeg software path"
                                .to_owned(),
                    };
                    (Some(fallback), Some(runtime_fallback), None)
                };

            return Ok(Box::new(MacosRuntimeAdapterInitializer {
                inner: native_inner,
                diagnostics,
                fallback,
                runtime_fallback,
                strict_frame_processor_error_prefix,
            }));
        }

        let diagnostics = macos_runtime_diagnostics(&media_info, &options);

        Ok(Box::new(MacosRuntimeAdapterInitializer {
            inner,
            diagnostics,
            fallback: None,
            runtime_fallback: None,
            strict_frame_processor_error_prefix: None,
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

    fn initialize(self: Box<Self>) -> PlayerResult<PlayerRuntimeAdapterBootstrap> {
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

    fn initialize(self: Box<Self>) -> PlayerResult<PlayerRuntimeAdapterBootstrap> {
        let Self {
            inner,
            diagnostics,
            fallback,
            runtime_fallback,
            strict_frame_processor_error_prefix,
        } = *self;

        match inner.initialize() {
            Ok(bootstrap) => Ok(wrap_macos_runtime_bootstrap(
                bootstrap,
                diagnostics,
                runtime_fallback,
            )),
            Err(native_error) => {
                let Some(fallback) = fallback else {
                    if let Some(prefix) = strict_frame_processor_error_prefix {
                        return Err(PlayerError::new(
                            native_error.code(),
                            format!("{prefix}: {}", native_error.message()),
                        ));
                    }
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
            plugin_diagnostics: diagnostics.plugin_diagnostics.clone(),
            has_video_surface: diagnostics.has_video_surface,
            runtime_fallback,
            pending_runtime_fallback_events: VecDeque::new(),
            source_normalizer_packet_session: None,
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
    ) -> PlayerResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
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
                PlayerRuntimeEvent::Initialized(startup) => {
                    let startup = apply_video_decode_diagnostics(startup, &self.video_decode);
                    PlayerRuntimeEvent::Initialized(append_plugin_diagnostics(
                        startup,
                        &self.plugin_diagnostics,
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
        command: PlayerRuntimeCommand,
    ) -> PlayerResult<PlayerRuntimeCommandResult> {
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

    fn advance(&mut self) -> PlayerResult<Option<DecodedVideoFrame>> {
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
    fn activate_runtime_fallback(&mut self, runtime_error_message: &str) -> PlayerResult<()> {
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
        ) -> PlayerResult<PlayerRuntimeAdapterBootstrap>,
    ) -> PlayerResult<()> {
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
        self.plugin_diagnostics = bootstrap.startup.plugin_diagnostics.clone();
        self.has_video_surface = false;
        self.pending_runtime_fallback_events
            .extend(runtime_fallback_events(runtime_error_message));

        Ok(())
    }
}

struct MacosSourceNormalizerRuntimeGuard {
    inner: PlayerRuntime,
    source_normalizer_packet_session:
        Option<Arc<Mutex<Option<Box<dyn SourceNormalizerPacketSession>>>>>,
    source_normalizer_diagnostics: Vec<PlayerPluginDiagnostic>,
}

impl PlayerRuntimeAdapter for MacosSourceNormalizerRuntimeGuard {
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

    fn progress(&self) -> PlaybackProgress {
        self.inner.progress()
    }

    fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
        self.inner
            .drain_events()
            .into_iter()
            .map(|event| match event {
                PlayerRuntimeEvent::Initialized(startup) => PlayerRuntimeEvent::Initialized(
                    append_plugin_diagnostics(startup, &self.source_normalizer_diagnostics),
                ),
                other => other,
            })
            .collect()
    }

    fn dispatch(
        &mut self,
        command: PlayerRuntimeCommand,
    ) -> PlayerResult<PlayerRuntimeCommandResult> {
        self.inner.dispatch(command)
    }

    fn replace_video_surface(
        &mut self,
        video_surface: Option<PlayerVideoSurfaceTarget>,
    ) -> PlayerResult<()> {
        self.inner.replace_video_surface(video_surface)
    }

    fn advance(&mut self) -> PlayerResult<Option<DecodedVideoFrame>> {
        self.inner.advance()
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.inner.next_deadline()
    }
}

impl Drop for MacosSourceNormalizerRuntimeGuard {
    fn drop(&mut self) {
        if let Some(session) = self.source_normalizer_packet_session.take()
            && let Ok(mut guard) = session.lock()
            && let Some(mut packet_session) = guard.take()
        {
            let _ = packet_session.close();
        }
    }
}

fn should_trigger_runtime_fallback_for_advance(error: &PlayerError) -> bool {
    if error.code() != PlayerErrorCode::BackendFailure {
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
    error: &PlayerError,
) -> bool {
    if error.code() != PlayerErrorCode::BackendFailure {
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

fn strict_frame_processor_selection(selection: &MacosNativeFrameDecoderSelection) -> bool {
    selection.frame_processor_mode == FrameProcessorMode::RequireProcessed
        && !selection.frame_processor_paths.is_empty()
}

#[derive(Debug, Clone)]
struct MacosNativeFrameDecoderSelection {
    plugin_path: PathBuf,
    plugin_name: Option<String>,
    video_surface: PlayerVideoSurfaceTarget,
    frame_processor_paths: Vec<PathBuf>,
    frame_processor_mode: FrameProcessorMode,
    frame_processor_policy: FrameProcessorPolicy,
}

#[derive(Debug)]
struct MacosNativeFrameVideoSourceFactory {
    plugin_path: PathBuf,
    video_surface: PlayerVideoSurfaceTarget,
    frame_processor_paths: Vec<PathBuf>,
    frame_processor_mode: FrameProcessorMode,
    frame_processor_policy: FrameProcessorPolicy,
}

struct MacosSourceNormalizerPacketVideoSourceFactory {
    decoder_plugin_path: PathBuf,
    decoder_plugin_name: Option<String>,
    video_surface: PlayerVideoSurfaceTarget,
    frame_processor_paths: Vec<PathBuf>,
    frame_processor_mode: FrameProcessorMode,
    frame_processor_policy: FrameProcessorPolicy,
    packet_session: Arc<Mutex<Option<Box<dyn SourceNormalizerPacketSession>>>>,
}

impl std::fmt::Debug for MacosSourceNormalizerPacketVideoSourceFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacosSourceNormalizerPacketVideoSourceFactory")
            .field("decoder_plugin_path", &self.decoder_plugin_path)
            .field("decoder_plugin_name", &self.decoder_plugin_name)
            .field("video_surface", &self.video_surface)
            .field("frame_processor_paths", &self.frame_processor_paths)
            .field("frame_processor_mode", &self.frame_processor_mode)
            .field("frame_processor_policy", &self.frame_processor_policy)
            .finish_non_exhaustive()
    }
}

struct MacosNativeFrameVideoSource {
    stream_info: VideoPacketStreamInfo,
    session: Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    shared: Arc<Mutex<MacosNativeFrameDecoderState>>,
    outstanding_frames: Arc<AtomicUsize>,
    command_tx: Sender<MacosNativeFrameWorkerCommand>,
    frame_rx: Receiver<MacosNativeFrameWorkerEvent>,
    generation: u64,
    current_generation: Arc<AtomicU64>,
    buffered_frame_count: Arc<AtomicUsize>,
    prefetch_limit: Arc<AtomicUsize>,
    prefetch_wakeup: Arc<MacosNativeFramePrefetchWakeup>,
    end_of_input_sent: bool,
    end_of_stream_received: bool,
    worker: Option<JoinHandle<()>>,
}

// Lock ordering for native-frame playback: acquire `session` before `shared` whenever both are
// needed. Holding `shared` while taking `session` can deadlock with decoder receive/release paths.
struct MacosNativeFrameDecoderState {
    frame_processor_chain: Option<MacosFrameProcessorChain>,
    presenter: Option<MacosMetalLayerPresenter>,
    presentation_epoch: u64,
}

#[derive(Debug, Default)]
struct MacosNativeFramePrefetchWakeup {
    state: Mutex<MacosNativeFramePrefetchWakeupState>,
    changed: Condvar,
}

#[derive(Debug, Default)]
struct MacosNativeFramePrefetchWakeupState {
    sequence: u64,
}

impl MacosNativeFramePrefetchWakeup {
    fn notify(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.sequence = state.sequence.wrapping_add(1);
            self.changed.notify_all();
        }
    }

    fn wait_for_change(&self, observed_sequence: &mut u64) {
        let Ok(state) = self.state.lock() else {
            return;
        };
        let sequence = state.sequence;
        if sequence != *observed_sequence {
            *observed_sequence = sequence;
            return;
        }
        if let Ok((state_after_wait, _)) = self
            .changed
            .wait_timeout(state, MACOS_NATIVE_FRAME_PREFETCH_COMMAND_POLL_INTERVAL)
        {
            *observed_sequence = state_after_wait.sequence;
        }
    }
}

#[derive(Debug)]
struct MacosFrameProcessorChain {
    processors: Vec<MacosFrameProcessorNode>,
    mode: FrameProcessorMode,
    policy: FrameProcessorPolicy,
    metrics: PlayerFrameProcessingMetrics,
    pending_events: VecDeque<PlayerRuntimeEvent>,
    debug: FrameProcessorDebugState,
}

struct MacosFrameProcessorNode {
    plugin_name: String,
    processor_index: usize,
    session: Box<dyn FrameProcessorSession>,
}

impl std::fmt::Debug for MacosFrameProcessorNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacosFrameProcessorNode")
            .field("plugin_name", &self.plugin_name)
            .field("processor_index", &self.processor_index)
            .finish()
    }
}

#[derive(Debug)]
struct MacosFrameProcessorFrame {
    decoder_frame: DecoderNativeFrame,
    presentation_frame: DecoderNativeFrame,
    processor_outputs: Vec<ProcessorOwnedNativeFrame>,
}

#[derive(Debug)]
struct ProcessorOwnedNativeFrame {
    processor_index: usize,
    frame: NativeFrame,
}

#[derive(Debug)]
struct FrameProcessorDebugState {
    enabled: bool,
    trace_frames: bool,
    window_frames: u64,
    frame_count: u64,
    window_start: Instant,
    last_pts_us: Option<i64>,
    last_frame_started_at: Option<Instant>,
    max_wall_us: u128,
    total_wall_us: u128,
    max_pts_delta_us: Option<i64>,
    total_pts_delta_us: i128,
    pts_delta_count: u64,
    bypassed_frames: u64,
    dropped_outputs: u64,
    deadline_misses: u64,
    backpressure_count: u64,
    pending_count: u64,
    presented_processed: u64,
    presented_original: u64,
    max_queue_depth: Option<u32>,
    max_in_flight_frames: Option<u32>,
}

impl FrameProcessorDebugState {
    // Debug environment variables are snapshotted when the processor chain is opened. Changing
    // them during playback requires recreating the player/runtime.
    fn from_env() -> Self {
        Self {
            enabled: env_flag(FRAME_PROCESSOR_DEBUG_ENV),
            trace_frames: env_flag(FRAME_PROCESSOR_DEBUG_TRACE_ENV),
            window_frames: env_u64(FRAME_PROCESSOR_DEBUG_WINDOW_ENV)
                .unwrap_or(DEFAULT_FRAME_PROCESSOR_DEBUG_WINDOW)
                .max(1),
            frame_count: 0,
            window_start: Instant::now(),
            last_pts_us: None,
            last_frame_started_at: None,
            max_wall_us: 0,
            total_wall_us: 0,
            max_pts_delta_us: None,
            total_pts_delta_us: 0,
            pts_delta_count: 0,
            bypassed_frames: 0,
            dropped_outputs: 0,
            deadline_misses: 0,
            backpressure_count: 0,
            pending_count: 0,
            presented_processed: 0,
            presented_original: 0,
            max_queue_depth: None,
            max_in_flight_frames: None,
        }
    }

    fn begin_frame(&mut self, pts_us: Option<i64>) -> FrameProcessorFrameDebugSample {
        if !self.enabled {
            return FrameProcessorFrameDebugSample::default();
        }
        self.frame_count = self.frame_count.saturating_add(1);
        let started_at = Instant::now();
        self.last_frame_started_at = Some(started_at);
        let pts_delta_us = pts_us.and_then(|pts| self.last_pts_us.map(|previous| pts - previous));
        if let Some(delta) = pts_delta_us {
            self.max_pts_delta_us = Some(
                self.max_pts_delta_us
                    .map(|current| current.max(delta.abs()))
                    .unwrap_or(delta.abs()),
            );
            self.total_pts_delta_us = self.total_pts_delta_us.saturating_add(delta as i128);
            self.pts_delta_count = self.pts_delta_count.saturating_add(1);
        }
        if pts_us.is_some() {
            self.last_pts_us = pts_us;
        }
        FrameProcessorFrameDebugSample {
            sequence: self.frame_count,
            started_at: Some(started_at),
            input_pts_us: pts_us,
            pts_delta_us,
            ..FrameProcessorFrameDebugSample::default()
        }
    }

    fn observe_submit(&mut self, queue_depth: Option<u32>, in_flight_frames: Option<u32>) {
        if !self.enabled {
            return;
        }
        self.max_queue_depth = max_option_u32(self.max_queue_depth, queue_depth);
        self.max_in_flight_frames = max_option_u32(self.max_in_flight_frames, in_flight_frames);
    }

    fn observe_bypass(&mut self) {
        if self.enabled {
            self.bypassed_frames = self.bypassed_frames.saturating_add(1);
        }
    }

    fn observe_backpressure(&mut self) {
        if self.enabled {
            self.backpressure_count = self.backpressure_count.saturating_add(1);
        }
    }

    fn observe_pending(&mut self) {
        if self.enabled {
            self.pending_count = self.pending_count.saturating_add(1);
        }
    }

    fn observe_deadline_miss(&mut self) {
        if self.enabled {
            self.deadline_misses = self.deadline_misses.saturating_add(1);
        }
    }

    fn observe_dropped_output(&mut self) {
        if self.enabled {
            self.dropped_outputs = self.dropped_outputs.saturating_add(1);
        }
    }

    fn finish_frame(&mut self, sample: FrameProcessorFrameDebugSample) {
        if !self.enabled {
            return;
        }
        let wall_us = sample
            .started_at
            .map(|started_at| started_at.elapsed().as_micros())
            .unwrap_or(0);
        self.max_wall_us = self.max_wall_us.max(wall_us);
        self.total_wall_us = self.total_wall_us.saturating_add(wall_us);
        if sample.presented_processed {
            self.presented_processed = self.presented_processed.saturating_add(1);
        } else {
            self.presented_original = self.presented_original.saturating_add(1);
        }
        if self.trace_frames {
            info!(
                sequence = sample.sequence,
                input_pts_us = sample.input_pts_us,
                pts_delta_us = sample.pts_delta_us,
                wall_us,
                node_count = sample.node_count,
                submitted_nodes = sample.submitted_nodes,
                processed_nodes = sample.processed_nodes,
                bypassed = sample.bypassed,
                pending = sample.pending,
                dropped_output = sample.dropped_output,
                deadline_missed = sample.deadline_missed,
                presented_processed = sample.presented_processed,
                output_pts_us = sample.output_pts_us,
                "macOS frame processor debug frame"
            );
        }
        if self.frame_count.is_multiple_of(self.window_frames) {
            self.log_summary();
            self.reset_window();
        }
    }

    fn log_summary(&self) {
        let avg_wall_us = if self.window_frames == 0 {
            0
        } else {
            self.total_wall_us / u128::from(self.window_frames)
        };
        let avg_pts_delta_us = if self.pts_delta_count == 0 {
            None
        } else {
            Some(self.total_pts_delta_us / i128::from(self.pts_delta_count))
        };
        info!(
            frames = self.window_frames,
            elapsed_ms = self.window_start.elapsed().as_millis(),
            avg_wall_us,
            max_wall_us = self.max_wall_us,
            avg_pts_delta_us,
            max_pts_delta_us = self.max_pts_delta_us,
            bypassed_frames = self.bypassed_frames,
            dropped_outputs = self.dropped_outputs,
            deadline_misses = self.deadline_misses,
            backpressure_count = self.backpressure_count,
            pending_count = self.pending_count,
            presented_processed = self.presented_processed,
            presented_original = self.presented_original,
            max_queue_depth = self.max_queue_depth,
            max_in_flight_frames = self.max_in_flight_frames,
            "macOS frame processor debug summary"
        );
    }

    fn reset_window(&mut self) {
        self.window_start = Instant::now();
        self.max_wall_us = 0;
        self.total_wall_us = 0;
        self.max_pts_delta_us = None;
        self.total_pts_delta_us = 0;
        self.pts_delta_count = 0;
        self.bypassed_frames = 0;
        self.dropped_outputs = 0;
        self.deadline_misses = 0;
        self.backpressure_count = 0;
        self.pending_count = 0;
        self.presented_processed = 0;
        self.presented_original = 0;
        self.max_queue_depth = None;
        self.max_in_flight_frames = None;
    }
}

#[derive(Debug, Default)]
struct FrameProcessorFrameDebugSample {
    sequence: u64,
    started_at: Option<Instant>,
    input_pts_us: Option<i64>,
    output_pts_us: Option<i64>,
    pts_delta_us: Option<i64>,
    node_count: usize,
    submitted_nodes: usize,
    processed_nodes: usize,
    bypassed: bool,
    pending: bool,
    dropped_output: bool,
    deadline_missed: bool,
    presented_processed: bool,
}

#[derive(Debug)]
struct MacosFrameProcessorProcessState {
    current_frame: NativeFrame,
    processor_outputs: Vec<ProcessorOwnedNativeFrame>,
    using_processor_output: bool,
    debug_sample: FrameProcessorFrameDebugSample,
}

#[derive(Debug)]
enum MacosNativeFramePoll {
    Frame(MacosFrameProcessorFrame),
    Decoder(DecoderReceiveNativeFrameOutput),
}

#[derive(Debug)]
enum MacosNativeFrameWorkerCommand {
    Seek { generation: u64, position: Duration },
    Shutdown,
}

#[derive(Debug)]
enum MacosNativeFrameWorkerEvent {
    Frame {
        generation: u64,
        frame: MacosFrameProcessorFrame,
    },
    EndOfStream {
        generation: u64,
    },
    Error {
        generation: u64,
        message: String,
    },
}

struct MacosDeferredNativeFramePresentation {
    session: Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    shared: Arc<Mutex<MacosNativeFrameDecoderState>>,
    outstanding_frames: Arc<AtomicUsize>,
    frame: Option<MacosFrameProcessorFrame>,
    presentation_epoch: u64,
}

impl std::fmt::Debug for MacosDeferredNativeFramePresentation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacosDeferredNativeFramePresentation")
            .field("has_frame", &self.frame.is_some())
            .field("presentation_epoch", &self.presentation_epoch)
            .finish()
    }
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

impl Drop for MacosNativeFrameVideoSource {
    fn drop(&mut self) {
        let _ = self
            .command_tx
            .send(MacosNativeFrameWorkerCommand::Shutdown);
        self.prefetch_wakeup.notify();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.release_queued_prefetch_events();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MacosNativeFramePacketSendStatus {
    Sent,
    NeedMoreData,
    EndOfStream,
}

trait MacosNativeFramePacketSource: Send {
    fn send_next_packet(
        &mut self,
        decoder_session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    ) -> anyhow::Result<MacosNativeFramePacketSendStatus>;
    fn seek_to(&mut self, position: Duration) -> anyhow::Result<()>;
}

impl MacosNativeFramePacketSource for VideoPacketSource {
    fn send_next_packet(
        &mut self,
        decoder_session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    ) -> anyhow::Result<MacosNativeFramePacketSendStatus> {
        match VideoPacketSource::next_packet(self)? {
            Some(packet) => {
                send_macos_native_frame_packet(decoder_session, packet)?;
                Ok(MacosNativeFramePacketSendStatus::Sent)
            }
            None => Ok(MacosNativeFramePacketSendStatus::EndOfStream),
        }
    }

    fn seek_to(&mut self, position: Duration) -> anyhow::Result<()> {
        VideoPacketSource::seek_to(self, position)
    }
}

struct SourceNormalizerPacketSource {
    session: Arc<Mutex<Option<Box<dyn SourceNormalizerPacketSession>>>>,
    pending: Option<SourceNormalizerPendingPacket>,
}

#[derive(Debug)]
struct SourceNormalizerPendingPacket {
    packet: DecoderPacket,
    data: Vec<u8>,
}

impl SourceNormalizerPacketSource {
    fn new(session: Arc<Mutex<Option<Box<dyn SourceNormalizerPacketSession>>>>) -> Self {
        Self {
            session,
            pending: None,
        }
    }

    fn send_pending_packet(
        &mut self,
        decoder_session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
        pending: SourceNormalizerPendingPacket,
    ) -> anyhow::Result<MacosNativeFramePacketSendStatus> {
        let send_result = send_macos_native_frame_packet_bytes(
            decoder_session,
            pending.packet.clone(),
            &pending.data,
        );
        match send_result {
            Ok(result) if result.accepted => Ok(MacosNativeFramePacketSendStatus::Sent),
            Ok(_) => {
                self.pending = Some(pending);
                Ok(MacosNativeFramePacketSendStatus::NeedMoreData)
            }
            Err(error) => Err(error),
        }
    }
}

impl Drop for SourceNormalizerPacketSource {
    fn drop(&mut self) {
        self.pending = None;
    }
}

impl MacosNativeFramePacketSource for SourceNormalizerPacketSource {
    fn send_next_packet(
        &mut self,
        decoder_session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    ) -> anyhow::Result<MacosNativeFramePacketSendStatus> {
        if let Some(pending) = self.pending.take() {
            return self.send_pending_packet(decoder_session, pending);
        }

        let session_arc = self.session.clone();
        let mut guard = session_arc
            .lock()
            .map_err(|_| anyhow::anyhow!("source normalizer packet session is poisoned"))?;
        let session = guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("source normalizer packet session is not configured"))?;
        let lease = session
            .read_packet()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        if lease.metadata.status == SourceNormalizerReadPacketStatus::EndOfStream {
            return Ok(MacosNativeFramePacketSendStatus::EndOfStream);
        }
        if lease.metadata.status == SourceNormalizerReadPacketStatus::NeedMoreData {
            return Ok(MacosNativeFramePacketSendStatus::NeedMoreData);
        }
        let metadata = source_normalizer_packet_metadata(&lease.metadata);
        let data = lease.data.to_vec();
        let handle = lease.handle;
        drop(lease);
        session
            .release_packet(handle)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let metadata = metadata?;
        let pending = SourceNormalizerPendingPacket {
            packet: metadata,
            data,
        };
        self.send_pending_packet(decoder_session, pending)
    }

    fn seek_to(&mut self, position: Duration) -> anyhow::Result<()> {
        self.pending = None;
        let mut guard = self
            .session
            .lock()
            .map_err(|_| anyhow::anyhow!("source normalizer packet session is poisoned"))?;
        let session = guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("source normalizer packet session is not configured"))?;
        session
            .seek(&SourceNormalizerPacketSeek {
                position_millis: position.as_millis().min(u64::MAX as u128) as u64,
                exact: false,
            })
            .map(|_| ())
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }
}

impl DesktopVideoSourceFactory for MacosSourceNormalizerPacketVideoSourceFactory {
    fn open_video_source(
        &self,
        source: MediaSource,
        _buffer_capacity: usize,
        _interrupt_flag: Option<Arc<AtomicBool>>,
    ) -> anyhow::Result<DesktopVideoSourceBootstrap> {
        let stream_info = {
            let guard = self
                .packet_session
                .lock()
                .map_err(|_| anyhow::anyhow!("source normalizer packet session is poisoned"))?;
            let session = guard.as_ref().ok_or_else(|| {
                anyhow::anyhow!("source normalizer packet session is not configured")
            })?;
            macos_packet_stream_info_from_source_normalizer(&session.stream_info())?
        };
        let plugin = LoadedDynamicPlugin::load(&self.decoder_plugin_path).with_context(|| {
            format!(
                "failed to load native-frame decoder plugin {}",
                self.decoder_plugin_path.display()
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
                bitstream_format: Some(macos_decoder_bitstream_format(&stream_info.codec)),
                width: stream_info.width,
                height: stream_info.height,
                coded_width: stream_info.width,
                coded_height: stream_info.height,
                prefer_hardware: true,
                require_cpu_output: false,
                ..DecoderSessionConfig::default()
            })
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let session_info = session.session_info();
        let presenter = MacosMetalLayerPresenter::new(self.video_surface)
            .map_err(|error| anyhow::anyhow!(error.message().to_owned()))?;
        let frame_processor_chain = open_macos_frame_processor_chain(
            &stream_info,
            &self.frame_processor_paths,
            self.frame_processor_mode,
            self.frame_processor_policy.clone(),
        )?;
        let decode_info = BackendVideoDecodeInfo {
            selected_mode: BackendVideoDecoderMode::Hardware,
            hardware_available: true,
            hardware_backend: session_info
                .selected_hardware_backend
                .or_else(|| Some(VIDEOTOOLBOX_BACKEND_NAME.to_owned())),
            decoder_name: session_info.decoder_name.unwrap_or_else(|| {
                self.decoder_plugin_name
                    .clone()
                    .unwrap_or_else(|| factory.name().to_owned())
            }),
            fallback_reason: None,
        };
        let probe = player_backend_ffmpeg::MediaProbe {
            source: source.clone(),
            duration: None,
            bit_rate: None,
            audio_streams: 0,
            video_streams: 1,
            best_video: Some(player_backend_ffmpeg::VideoStreamProbe {
                index: stream_info.stream_index,
                codec: stream_info.codec.clone(),
                width: stream_info.width.unwrap_or_default(),
                height: stream_info.height.unwrap_or_default(),
                frame_rate: stream_info.frame_rate,
            }),
            best_audio: None,
        };
        let outstanding_frames = Arc::new(AtomicUsize::new(0));
        let session = Arc::new(Mutex::new(session));
        let shared = Arc::new(Mutex::new(MacosNativeFrameDecoderState {
            frame_processor_chain,
            presenter: Some(presenter),
            presentation_epoch: 0,
        }));
        let (command_tx, command_rx) = mpsc::channel();
        let (frame_tx, frame_rx) = mpsc::channel();
        let current_generation = Arc::new(AtomicU64::new(0));
        let buffered_frame_count = Arc::new(AtomicUsize::new(0));
        let prefetch_limit = Arc::new(AtomicUsize::new(1));
        let prefetch_wakeup = Arc::new(MacosNativeFramePrefetchWakeup::default());
        let worker = spawn_macos_native_frame_prefetch_worker(
            Box::new(SourceNormalizerPacketSource::new(
                self.packet_session.clone(),
            )),
            session.clone(),
            shared.clone(),
            outstanding_frames.clone(),
            command_rx,
            frame_tx,
            current_generation.clone(),
            buffered_frame_count.clone(),
            prefetch_limit.clone(),
            prefetch_wakeup.clone(),
        )?;

        Ok(DesktopVideoSourceBootstrap {
            source: Box::new(MacosNativeFrameVideoSource {
                stream_info,
                session,
                shared,
                outstanding_frames,
                command_tx,
                frame_rx,
                generation: 0,
                current_generation,
                buffered_frame_count,
                prefetch_limit,
                prefetch_wakeup,
                end_of_input_sent: false,
                end_of_stream_received: false,
                worker: Some(worker),
            }),
            decode_info,
            probe,
        })
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
                bitstream_format: Some(macos_decoder_bitstream_format(&stream_info.codec)),
                width: stream_info.width,
                height: stream_info.height,
                coded_width: stream_info.width,
                coded_height: stream_info.height,
                prefer_hardware: true,
                require_cpu_output: false,
                ..DecoderSessionConfig::default()
            })
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let session_info = session.session_info();
        let presenter = MacosMetalLayerPresenter::new(self.video_surface)
            .map_err(|error| anyhow::anyhow!(error.message().to_owned()))?;
        let frame_processor_chain = open_macos_frame_processor_chain(
            &stream_info,
            &self.frame_processor_paths,
            self.frame_processor_mode,
            self.frame_processor_policy.clone(),
        )?;
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
        let outstanding_frames = Arc::new(AtomicUsize::new(0));
        let session = Arc::new(Mutex::new(session));
        let shared = Arc::new(Mutex::new(MacosNativeFrameDecoderState {
            frame_processor_chain,
            presenter: Some(presenter),
            presentation_epoch: 0,
        }));
        let (command_tx, command_rx) = mpsc::channel();
        let (frame_tx, frame_rx) = mpsc::channel();
        let current_generation = Arc::new(AtomicU64::new(0));
        let buffered_frame_count = Arc::new(AtomicUsize::new(0));
        let prefetch_limit = Arc::new(AtomicUsize::new(1));
        let prefetch_wakeup = Arc::new(MacosNativeFramePrefetchWakeup::default());
        let worker = spawn_macos_native_frame_prefetch_worker(
            Box::new(packet_source),
            session.clone(),
            shared.clone(),
            outstanding_frames.clone(),
            command_rx,
            frame_tx,
            current_generation.clone(),
            buffered_frame_count.clone(),
            prefetch_limit.clone(),
            prefetch_wakeup.clone(),
        )?;

        Ok(DesktopVideoSourceBootstrap {
            source: Box::new(MacosNativeFrameVideoSource {
                stream_info,
                session,
                shared,
                outstanding_frames,
                command_tx,
                frame_rx,
                generation: 0,
                current_generation,
                buffered_frame_count,
                prefetch_limit,
                prefetch_wakeup,
                end_of_input_sent: false,
                end_of_stream_received: false,
                worker: Some(worker),
            }),
            decode_info,
            probe,
        })
    }
}

impl DesktopVideoSource for MacosNativeFrameVideoSource {
    fn recv_frame(&mut self) -> anyhow::Result<Option<DesktopVideoFrame>> {
        self.recv_prefetched_frame()
    }

    fn try_recv_frame(&mut self) -> anyhow::Result<DesktopVideoFramePoll> {
        self.try_recv_prefetched_frame()
    }

    fn seek_to(&mut self, position: Duration) -> anyhow::Result<Option<DesktopVideoFrame>> {
        {
            let mut shared = self
                .shared
                .lock()
                .map_err(|_| anyhow::anyhow!("native-frame decoder state is poisoned"))?;
            shared.presentation_epoch = shared.presentation_epoch.saturating_add(1);
        }
        self.generation = self.generation.wrapping_add(1);
        self.current_generation
            .store(self.generation, Ordering::SeqCst);
        self.buffered_frame_count.store(0, Ordering::SeqCst);
        self.end_of_input_sent = false;
        self.end_of_stream_received = false;
        self.command_tx
            .send(MacosNativeFrameWorkerCommand::Seek {
                generation: self.generation,
                position,
            })
            .context("failed to send seek request to macOS native-frame prefetch worker")?;
        self.prefetch_wakeup.notify();
        self.recv_prefetched_frame()
    }

    fn buffered_frame_count(&self) -> usize {
        self.buffered_frame_count.load(Ordering::SeqCst)
    }

    fn set_prefetch_limit(&self, limit: usize) {
        self.prefetch_limit.store(limit.max(1), Ordering::SeqCst);
        self.prefetch_wakeup.notify();
    }

    fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
        self.shared
            .lock()
            .ok()
            .and_then(|mut shared| {
                shared
                    .frame_processor_chain
                    .as_mut()
                    .map(MacosFrameProcessorChain::drain_events)
            })
            .unwrap_or_default()
    }
}

impl MacosNativeFrameVideoSource {
    fn recv_prefetched_frame(&mut self) -> anyhow::Result<Option<DesktopVideoFrame>> {
        loop {
            if self.end_of_input_sent {
                return Ok(None);
            }

            let event = self
                .frame_rx
                .recv()
                .context("macOS native-frame prefetch worker disconnected")?;
            if let Some(frame) = self.handle_prefetch_event(event)? {
                return Ok(Some(frame));
            }

            if self.end_of_input_sent {
                return Ok(None);
            }
        }
    }

    fn try_recv_prefetched_frame(&mut self) -> anyhow::Result<DesktopVideoFramePoll> {
        if self.end_of_input_sent {
            return Ok(DesktopVideoFramePoll::EndOfStream);
        }

        loop {
            match self.frame_rx.try_recv() {
                Ok(event) => {
                    if let Some(frame) = self.handle_prefetch_event(event)? {
                        return Ok(DesktopVideoFramePoll::Ready(frame));
                    }
                    if self.end_of_input_sent {
                        return Ok(DesktopVideoFramePoll::EndOfStream);
                    }
                }
                Err(TryRecvError::Empty) => return Ok(DesktopVideoFramePoll::Pending),
                Err(TryRecvError::Disconnected) => {
                    anyhow::bail!("macOS native-frame prefetch worker disconnected")
                }
            }
        }
    }

    fn handle_prefetch_event(
        &mut self,
        event: MacosNativeFrameWorkerEvent,
    ) -> anyhow::Result<Option<DesktopVideoFrame>> {
        match event {
            MacosNativeFrameWorkerEvent::Frame { generation, frame }
                if generation == self.generation =>
            {
                decrement_macos_native_frame_buffered_count(
                    &self.buffered_frame_count,
                    &self.prefetch_wakeup,
                );
                self.deferred_desktop_frame(frame).map(Some)
            }
            MacosNativeFrameWorkerEvent::Frame { frame, .. } => {
                decrement_macos_native_frame_buffered_count(
                    &self.buffered_frame_count,
                    &self.prefetch_wakeup,
                );
                if let (Ok(mut session), Ok(mut shared)) = (self.session.lock(), self.shared.lock())
                {
                    let _ = release_macos_processor_frame_and_track(
                        session.as_mut(),
                        &mut shared,
                        self.outstanding_frames.as_ref(),
                        frame,
                    );
                }
                Ok(None)
            }
            MacosNativeFrameWorkerEvent::EndOfStream { generation }
                if generation == self.generation =>
            {
                self.end_of_input_sent = true;
                self.end_of_stream_received = true;
                Ok(None)
            }
            MacosNativeFrameWorkerEvent::Error {
                generation,
                message,
            } if generation == self.generation => Err(anyhow::anyhow!(message)),
            _ => Ok(None),
        }
    }

    fn release_queued_prefetch_events(&mut self) {
        while let Ok(event) = self.frame_rx.try_recv() {
            if let MacosNativeFrameWorkerEvent::Frame { frame, .. } = event {
                decrement_macos_native_frame_buffered_count(
                    &self.buffered_frame_count,
                    &self.prefetch_wakeup,
                );
                if let (Ok(mut session), Ok(mut shared)) = (self.session.lock(), self.shared.lock())
                {
                    let _ = release_macos_processor_frame_and_track(
                        session.as_mut(),
                        &mut shared,
                        self.outstanding_frames.as_ref(),
                        frame,
                    );
                }
            }
        }
    }

    fn deferred_desktop_frame(
        &self,
        frame: MacosFrameProcessorFrame,
    ) -> anyhow::Result<DesktopVideoFrame> {
        if frame.presentation_frame.metadata.handle_kind != DecoderNativeHandleKind::CvPixelBuffer {
            let mut session = self
                .session
                .lock()
                .map_err(|_| anyhow::anyhow!("native-frame decoder session is poisoned"))?;
            let mut shared = self
                .shared
                .lock()
                .map_err(|_| anyhow::anyhow!("native-frame decoder state is poisoned"))?;
            let _ = release_macos_processor_frame_and_track(
                session.as_mut(),
                &mut shared,
                self.outstanding_frames.as_ref(),
                frame,
            );
            anyhow::bail!("macOS native-frame presenter only accepts CVPixelBuffer handles");
        }
        let presentation_time = frame
            .presentation_frame
            .metadata
            .pts_us
            .and_then(duration_from_micros)
            .unwrap_or(Duration::ZERO);
        let width = frame.presentation_frame.metadata.width;
        let height = frame.presentation_frame.metadata.height;
        Ok(DesktopVideoFrame::native_deferred(
            presentation_time,
            width,
            height,
            Box::new(MacosDeferredNativeFramePresentation {
                session: self.session.clone(),
                shared: self.shared.clone(),
                outstanding_frames: self.outstanding_frames.clone(),
                frame: Some(frame),
                presentation_epoch: shared_presentation_epoch(&self.shared)?,
            }),
        ))
    }
}

fn spawn_macos_native_frame_prefetch_worker(
    packet_source: Box<dyn MacosNativeFramePacketSource>,
    session: Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    shared: Arc<Mutex<MacosNativeFrameDecoderState>>,
    outstanding_frames: Arc<AtomicUsize>,
    command_rx: Receiver<MacosNativeFrameWorkerCommand>,
    frame_tx: Sender<MacosNativeFrameWorkerEvent>,
    current_generation: Arc<AtomicU64>,
    buffered_frame_count: Arc<AtomicUsize>,
    prefetch_limit: Arc<AtomicUsize>,
    prefetch_wakeup: Arc<MacosNativeFramePrefetchWakeup>,
) -> anyhow::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("macos-native-frame-prefetch".to_owned())
        .spawn(move || {
            macos_native_frame_prefetch_worker_loop(
                packet_source,
                session,
                shared,
                outstanding_frames,
                command_rx,
                frame_tx,
                current_generation,
                buffered_frame_count,
                prefetch_limit,
                prefetch_wakeup,
            );
        })
        .context("failed to spawn macOS native-frame prefetch worker")
}

fn macos_native_frame_prefetch_worker_loop(
    mut packet_source: Box<dyn MacosNativeFramePacketSource>,
    session: Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    shared: Arc<Mutex<MacosNativeFrameDecoderState>>,
    outstanding_frames: Arc<AtomicUsize>,
    command_rx: Receiver<MacosNativeFrameWorkerCommand>,
    frame_tx: Sender<MacosNativeFrameWorkerEvent>,
    current_generation: Arc<AtomicU64>,
    buffered_frame_count: Arc<AtomicUsize>,
    prefetch_limit: Arc<AtomicUsize>,
    prefetch_wakeup: Arc<MacosNativeFramePrefetchWakeup>,
) {
    let mut generation = 0u64;
    let mut end_of_input_sent = false;
    let mut end_of_stream_received = false;
    let mut pending_event = None;
    let mut wakeup_sequence = 0u64;

    loop {
        match latest_macos_native_frame_worker_command(&command_rx) {
            Some(MacosNativeFrameWorkerCommand::Shutdown) => break,
            Some(MacosNativeFrameWorkerCommand::Seek {
                generation: new_generation,
                position,
            }) => {
                generation = new_generation;
                pending_event = None;
                end_of_input_sent = false;
                end_of_stream_received = false;
                let seek_result = flush_and_seek_macos_native_frame_source(
                    &session,
                    &shared,
                    packet_source.as_mut(),
                    position,
                );
                if let Err(error) = seek_result {
                    pending_event = Some(MacosNativeFrameWorkerEvent::Error {
                        generation,
                        message: error.to_string(),
                    });
                }
            }
            None => {}
        }

        if pending_event.is_none() {
            if end_of_stream_received {
                wait_for_macos_native_frame_prefetch_work(&prefetch_wakeup, &mut wakeup_sequence);
                continue;
            }
            let limit = prefetch_limit.load(Ordering::SeqCst).max(1);
            if buffered_frame_count.load(Ordering::SeqCst) >= limit {
                wait_for_macos_native_frame_prefetch_work(&prefetch_wakeup, &mut wakeup_sequence);
                continue;
            }
            pending_event = Some(
                match decode_next_macos_native_frame_worker_event(
                    &shared,
                    &session,
                    &outstanding_frames,
                    packet_source.as_mut(),
                    generation,
                    &mut end_of_input_sent,
                    &mut end_of_stream_received,
                ) {
                    Ok(event) => event,
                    Err(error) => MacosNativeFrameWorkerEvent::Error {
                        generation,
                        message: error.to_string(),
                    },
                },
            );
        }

        let Some(event) = pending_event.take() else {
            continue;
        };
        let frame_generation = macos_native_frame_worker_frame_generation(&event);
        if let Some(event_generation) = frame_generation
            && event_generation == current_generation.load(Ordering::SeqCst)
        {
            buffered_frame_count.fetch_add(1, Ordering::SeqCst);
        }
        match frame_tx.send(event) {
            Ok(()) => {}
            Err(event) => {
                if let Some(event_generation) = frame_generation
                    && event_generation == current_generation.load(Ordering::SeqCst)
                {
                    decrement_macos_native_frame_buffered_count(
                        &buffered_frame_count,
                        &prefetch_wakeup,
                    );
                }
                if let MacosNativeFrameWorkerEvent::Frame { frame, .. } = event.0
                    && let (Ok(mut session), Ok(mut shared)) = (session.lock(), shared.lock())
                {
                    let _ = release_macos_processor_frame_and_track(
                        session.as_mut(),
                        &mut shared,
                        outstanding_frames.as_ref(),
                        frame,
                    );
                }
                break;
            }
        }
    }
}

fn latest_macos_native_frame_worker_command(
    command_rx: &Receiver<MacosNativeFrameWorkerCommand>,
) -> Option<MacosNativeFrameWorkerCommand> {
    let mut latest = None;
    loop {
        match command_rx.try_recv() {
            Ok(MacosNativeFrameWorkerCommand::Shutdown) => {
                return Some(MacosNativeFrameWorkerCommand::Shutdown);
            }
            Ok(command) => latest = Some(command),
            Err(TryRecvError::Empty) => return latest,
            Err(TryRecvError::Disconnected) => {
                return Some(MacosNativeFrameWorkerCommand::Shutdown);
            }
        }
    }
}

fn wait_for_macos_native_frame_prefetch_work(
    wakeup: &MacosNativeFramePrefetchWakeup,
    observed_sequence: &mut u64,
) {
    wakeup.wait_for_change(observed_sequence);
}

fn macos_native_frame_worker_frame_generation(event: &MacosNativeFrameWorkerEvent) -> Option<u64> {
    match event {
        MacosNativeFrameWorkerEvent::Frame { generation, .. } => Some(*generation),
        _ => None,
    }
}

fn decrement_macos_native_frame_buffered_count(
    buffered_frame_count: &AtomicUsize,
    wakeup: &MacosNativeFramePrefetchWakeup,
) {
    let _ = buffered_frame_count.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
        Some(count.saturating_sub(1))
    });
    wakeup.notify();
}

fn flush_and_seek_macos_native_frame_source(
    session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    shared: &Arc<Mutex<MacosNativeFrameDecoderState>>,
    packet_source: &mut dyn MacosNativeFramePacketSource,
    position: Duration,
) -> anyhow::Result<()> {
    {
        let mut session = session
            .lock()
            .map_err(|_| anyhow::anyhow!("native-frame decoder session is poisoned"))?;
        session
            .flush()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    }
    {
        let mut shared = shared
            .lock()
            .map_err(|_| anyhow::anyhow!("native-frame decoder state is poisoned"))?;
        if let Some(chain) = shared.frame_processor_chain.as_mut() {
            chain.flush();
        }
    }
    packet_source.seek_to(position)
}

fn decode_next_macos_native_frame_worker_event(
    shared: &Arc<Mutex<MacosNativeFrameDecoderState>>,
    session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    outstanding_frames: &AtomicUsize,
    packet_source: &mut dyn MacosNativeFramePacketSource,
    generation: u64,
    end_of_input_sent: &mut bool,
    end_of_stream_received: &mut bool,
) -> anyhow::Result<MacosNativeFrameWorkerEvent> {
    loop {
        match receive_macos_native_frame_from_decoder(shared, session, outstanding_frames)? {
            MacosNativeFramePoll::Frame(frame) => {
                return Ok(MacosNativeFrameWorkerEvent::Frame { generation, frame });
            }
            MacosNativeFramePoll::Decoder(DecoderReceiveNativeFrameOutput::Eof) => {
                *end_of_stream_received = true;
                return Ok(MacosNativeFrameWorkerEvent::EndOfStream { generation });
            }
            MacosNativeFramePoll::Decoder(DecoderReceiveNativeFrameOutput::NeedMoreInput) => {}
            MacosNativeFramePoll::Decoder(DecoderReceiveNativeFrameOutput::Frame(_)) => {}
        }

        if *end_of_input_sent {
            thread::sleep(MACOS_NATIVE_FRAME_DECODER_DRAIN_RETRY_INTERVAL);
            continue;
        }

        match packet_source.send_next_packet(session)? {
            MacosNativeFramePacketSendStatus::Sent => {}
            MacosNativeFramePacketSendStatus::NeedMoreData => {
                thread::sleep(MACOS_NATIVE_FRAME_DECODER_DRAIN_RETRY_INTERVAL);
            }
            MacosNativeFramePacketSendStatus::EndOfStream => {
                send_macos_native_frame_end_of_stream(session)?;
                *end_of_input_sent = true;
            }
        }
    }
}

fn receive_macos_native_frame_from_decoder(
    shared: &Arc<Mutex<MacosNativeFrameDecoderState>>,
    session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    outstanding_frames: &AtomicUsize,
) -> anyhow::Result<MacosNativeFramePoll> {
    let mut session = session
        .lock()
        .map_err(|_| anyhow::anyhow!("native-frame decoder session is poisoned"))?;
    let result = session
        .receive_native_frame()
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let DecoderReceiveNativeFrameOutput::Frame(frame) = result else {
        return Ok(MacosNativeFramePoll::Decoder(result));
    };
    outstanding_frames.fetch_add(1, Ordering::SeqCst);
    let mut shared = shared
        .lock()
        .map_err(|_| anyhow::anyhow!("native-frame decoder state is poisoned"))?;
    let frame = match process_macos_native_frame(&mut shared, frame) {
        Ok(frame) => frame,
        Err((error, frame_for_release)) => {
            let _ = release_native_frame_with_counter(
                session.as_mut(),
                outstanding_frames,
                frame_for_release,
            );
            return Err(error);
        }
    };
    Ok(MacosNativeFramePoll::Frame(frame))
}

fn send_macos_native_frame_packet(
    session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    packet: CompressedVideoPacket,
) -> anyhow::Result<()> {
    send_macos_native_frame_packet_bytes(
        session,
        DecoderPacket {
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
    .map(|_| ())
}

fn source_normalizer_packet_metadata(
    metadata: &SourceNormalizerReadPacketMetadata,
) -> anyhow::Result<DecoderPacket> {
    let packet = metadata
        .packet
        .clone()
        .ok_or_else(|| anyhow::anyhow!("source normalizer packet metadata was missing"))?;
    Ok(DecoderPacket {
        pts_us: packet.pts_us,
        dts_us: packet.dts_us,
        duration_us: packet.duration_us,
        stream_index: packet.stream_index,
        key_frame: packet.key_frame,
        discontinuity: packet.discontinuity,
        end_of_stream: packet.end_of_stream,
    })
}

fn send_macos_native_frame_packet_bytes(
    session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    packet: DecoderPacket,
    data: &[u8],
) -> anyhow::Result<player_plugin::DecoderPacketResult> {
    let mut session = session
        .lock()
        .map_err(|_| anyhow::anyhow!("native-frame decoder session is poisoned"))?;
    session
        .send_packet(&packet, data)
        .map_err(|error| anyhow::anyhow!(error.to_string()))
}

fn send_macos_native_frame_end_of_stream(
    session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
) -> anyhow::Result<()> {
    let mut session = session
        .lock()
        .map_err(|_| anyhow::anyhow!("native-frame decoder session is poisoned"))?;
    session
        .send_packet(
            &DecoderPacket {
                end_of_stream: true,
                ..DecoderPacket::default()
            },
            &[],
        )
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!(error.to_string()))
}

impl DesktopVideoFramePresentation for MacosDeferredNativeFramePresentation {
    fn present(mut self: Box<Self>) -> anyhow::Result<()> {
        let Some(frame) = self.frame.take() else {
            return Ok(());
        };
        present_and_release_macos_processor_frame(
            &self.session,
            &self.shared,
            self.outstanding_frames.as_ref(),
            frame,
            self.presentation_epoch,
        )
    }
}

impl Drop for MacosDeferredNativeFramePresentation {
    fn drop(&mut self) {
        if let Some(frame) = self.frame.take()
            && let (Ok(mut session), Ok(mut shared)) = (self.session.lock(), self.shared.lock())
        {
            let _ = release_macos_processor_frame_and_track(
                session.as_mut(),
                &mut shared,
                self.outstanding_frames.as_ref(),
                frame,
            );
        }
    }
}

fn release_macos_processor_frame_and_track(
    session: &mut dyn NativeDecoderSession,
    shared: &mut MacosNativeFrameDecoderState,
    outstanding_frames: &AtomicUsize,
    frame: MacosFrameProcessorFrame,
) -> anyhow::Result<()> {
    if let Some(chain) = shared.frame_processor_chain.as_mut() {
        chain.release_processor_outputs(frame.processor_outputs);
    }
    release_native_frame_with_counter(session, outstanding_frames, frame.decoder_frame)
        .map_err(|error| anyhow::anyhow!(error.to_string()))
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

fn present_and_release_macos_processor_frame(
    session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
    shared: &Arc<Mutex<MacosNativeFrameDecoderState>>,
    outstanding_frames: &AtomicUsize,
    frame: MacosFrameProcessorFrame,
    presentation_epoch: u64,
) -> anyhow::Result<()> {
    let mut session = session
        .lock()
        .map_err(|_| anyhow::anyhow!("native-frame decoder session is poisoned"))?;
    let mut shared = shared
        .lock()
        .map_err(|_| anyhow::anyhow!("native-frame decoder state is poisoned"))?;
    if shared.presentation_epoch != presentation_epoch {
        return release_macos_processor_frame_and_track(
            session.as_mut(),
            &mut shared,
            outstanding_frames,
            frame,
        );
    }
    let presenter = shared
        .presenter
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("macOS native-frame presenter is not configured"))?;
    let present_result = presenter
        .present_cv_pixel_buffer_handle(frame.presentation_frame.handle)
        .map_err(|error| anyhow::anyhow!(error.message().to_owned()));
    let release_result = release_macos_processor_frame_and_track(
        session.as_mut(),
        &mut shared,
        outstanding_frames,
        frame,
    );
    present_result.and(release_result)
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

#[cfg(test)]
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

fn open_macos_frame_processor_chain(
    stream_info: &VideoPacketStreamInfo,
    paths: &[PathBuf],
    mode: FrameProcessorMode,
    policy: FrameProcessorPolicy,
) -> anyhow::Result<Option<MacosFrameProcessorChain>> {
    if mode == FrameProcessorMode::Disabled || paths.is_empty() {
        return Ok(None);
    }
    let input_metadata = NativeFrameMetadata {
        media_kind: DecoderMediaKind::Video,
        format: player_plugin::DecoderFrameFormat::Nv12,
        codec: stream_info.codec.clone(),
        pts_us: None,
        duration_us: None,
        width: stream_info.width.unwrap_or(0),
        height: stream_info.height.unwrap_or(0),
        coded_width: stream_info.width,
        coded_height: stream_info.height,
        visible_rect: None,
        handle_kind: NativeHandleKind::CvPixelBuffer,
        frame_id: None,
        release_tracking: None,
    };
    let mut processors = Vec::new();
    for (processor_index, path) in paths.iter().enumerate().take(policy.max_chain_depth) {
        let plugin = LoadedDynamicPlugin::load(path)
            .with_context(|| format!("failed to load frame processor plugin {}", path.display()))?;
        let factory = plugin.frame_processor_plugin_factory().ok_or_else(|| {
            anyhow::anyhow!(
                "plugin `{}` does not export a frame processor API",
                plugin.plugin_name()
            )
        })?;
        let capabilities = factory.capabilities();
        if !capabilities.supports_video_frames {
            anyhow::bail!(
                "frame processor `{}` does not support video frames",
                factory.name()
            );
        }
        if capabilities.may_change_dimensions {
            anyhow::bail!(
                "frame processor `{}` changes frame dimensions, which v1 does not allow",
                factory.name()
            );
        }
        let session = factory
            .open_session(&FrameProcessorSessionConfig {
                processor_index,
                input_metadata: input_metadata.clone(),
                max_in_flight_frames: Some(policy.max_in_flight_frames_per_processor),
            })
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        processors.push(MacosFrameProcessorNode {
            plugin_name: factory.name().to_owned(),
            processor_index,
            session,
        });
    }
    if processors.is_empty() {
        return Ok(None);
    }
    Ok(Some(MacosFrameProcessorChain {
        processors,
        mode,
        policy,
        metrics: PlayerFrameProcessingMetrics::default(),
        pending_events: VecDeque::new(),
        debug: FrameProcessorDebugState::from_env(),
    }))
}

fn process_macos_native_frame(
    shared: &mut MacosNativeFrameDecoderState,
    frame: DecoderNativeFrame,
) -> Result<MacosFrameProcessorFrame, (anyhow::Error, DecoderNativeFrame)> {
    let Some(chain) = shared.frame_processor_chain.as_mut() else {
        return Ok(MacosFrameProcessorFrame {
            decoder_frame: frame.clone(),
            presentation_frame: frame,
            processor_outputs: Vec::new(),
        });
    };
    chain.process(frame)
}

fn decoder_frame_to_native_frame(frame: &DecoderNativeFrame) -> NativeFrame {
    NativeFrame {
        metadata: frame.metadata.clone().into(),
        handle: frame.handle,
    }
}

fn native_frame_to_decoder_frame(frame: &NativeFrame) -> DecoderNativeFrame {
    DecoderNativeFrame {
        metadata: frame.metadata.clone().into(),
        handle: frame.handle,
    }
}

fn output_frame_requires_processor_release(frame: &NativeFrame) -> bool {
    frame
        .metadata
        .release_tracking
        .as_ref()
        .is_none_or(|tracking| tracking.requires_release)
}

impl MacosFrameProcessorChain {
    fn process(
        &mut self,
        decoder_frame: DecoderNativeFrame,
    ) -> Result<MacosFrameProcessorFrame, (anyhow::Error, DecoderNativeFrame)> {
        let mut state = self.begin_process_state(&decoder_frame);
        for node_index in 0..self.processors.len() {
            self.process_node(node_index, &decoder_frame, &mut state)?;
        }

        Ok(self.finish_process_state(decoder_frame, state))
    }

    fn begin_process_state(
        &mut self,
        decoder_frame: &DecoderNativeFrame,
    ) -> MacosFrameProcessorProcessState {
        let current_frame = decoder_frame_to_native_frame(decoder_frame);
        let mut debug_sample = self.debug.begin_frame(current_frame.metadata.pts_us);
        debug_sample.node_count = self.processors.len();
        MacosFrameProcessorProcessState {
            current_frame,
            processor_outputs: Vec::new(),
            using_processor_output: false,
            debug_sample,
        }
    }

    fn process_node(
        &mut self,
        node_index: usize,
        decoder_frame: &DecoderNativeFrame,
        state: &mut MacosFrameProcessorProcessState,
    ) -> Result<(), (anyhow::Error, DecoderNativeFrame)> {
        let submit_result = match self.submit_to_node(node_index, &state.current_frame) {
            Ok(result) => result,
            Err(error) => {
                self.release_processor_outputs(std::mem::take(&mut state.processor_outputs));
                return Err((error, decoder_frame.clone()));
            }
        };

        if self.handle_submit_status(node_index, submit_result, decoder_frame, state)? {
            return Ok(());
        }

        let receive_output = match self.receive_from_node(node_index) {
            Ok(output) => output,
            Err(error) => {
                self.release_processor_outputs(std::mem::take(&mut state.processor_outputs));
                return Err((error, decoder_frame.clone()));
            }
        };
        self.handle_receive_output(node_index, receive_output, decoder_frame, state)
    }

    fn submit_to_node(
        &mut self,
        node_index: usize,
        current_frame: &NativeFrame,
    ) -> anyhow::Result<FrameProcessorSubmitResult> {
        let submit = FrameProcessorSubmitFrame {
            metadata: current_frame.metadata.clone(),
            present_deadline_us: current_frame
                .metadata
                .pts_us
                .map(|pts| pts.saturating_add(duration_us_i64(self.policy.frame_deadline))),
        };
        self.metrics.submitted_frame_count = self.metrics.submitted_frame_count.saturating_add(1);
        let node = &mut self.processors[node_index];
        node.session
            .submit_frame(current_frame, &submit)
            .map_err(|error| {
                frame_processor_runtime_error(
                    self.mode,
                    node.processor_index,
                    &node.plugin_name,
                    error,
                )
            })
    }

    fn handle_submit_status(
        &mut self,
        node_index: usize,
        submit_result: FrameProcessorSubmitResult,
        decoder_frame: &DecoderNativeFrame,
        state: &mut MacosFrameProcessorProcessState,
    ) -> Result<bool, (anyhow::Error, DecoderNativeFrame)> {
        self.debug
            .observe_submit(submit_result.queue_depth, submit_result.in_flight_frames);
        match submit_result.status {
            FrameProcessorSubmitStatus::Accepted => {
                state.debug_sample.submitted_nodes =
                    state.debug_sample.submitted_nodes.saturating_add(1);
                Ok(false)
            }
            FrameProcessorSubmitStatus::Bypassed | FrameProcessorSubmitStatus::Backpressure => {
                self.handle_submit_bypass(node_index, submit_result, decoder_frame, state)?;
                Ok(true)
            }
            FrameProcessorSubmitStatus::Rejected => {
                self.handle_submit_rejected(node_index, submit_result, decoder_frame, state)?;
                Ok(true)
            }
        }
    }

    fn handle_submit_bypass(
        &mut self,
        node_index: usize,
        submit_result: FrameProcessorSubmitResult,
        decoder_frame: &DecoderNativeFrame,
        state: &mut MacosFrameProcessorProcessState,
    ) -> Result<(), (anyhow::Error, DecoderNativeFrame)> {
        self.reset_to_decoder_frame(decoder_frame, state);
        self.metrics.bypassed_frame_count = self.metrics.bypassed_frame_count.saturating_add(1);
        self.debug.observe_bypass();
        state.debug_sample.bypassed = true;
        if submit_result.status == FrameProcessorSubmitStatus::Backpressure {
            self.metrics.backpressure_count = self.metrics.backpressure_count.saturating_add(1);
            self.debug.observe_backpressure();
        }
        let node_snapshot = self.node_snapshot(node_index);
        let warning_kind = if submit_result.status == FrameProcessorSubmitStatus::Backpressure {
            FrameProcessorWarningKind::Backpressure
        } else {
            FrameProcessorWarningKind::BypassActivated
        };
        self.push_warning(
            warning_kind,
            &node_snapshot,
            &state.current_frame,
            FrameProcessorWarningDetails {
                queue_depth: submit_result.queue_depth,
                in_flight_frames: submit_result.in_flight_frames,
                ..FrameProcessorWarningDetails::default()
            },
            FrameProcessorPolicyAction::BypassOriginalFrame,
            submit_result.message,
        );
        if self.mode == FrameProcessorMode::RequireProcessed {
            return Err((
                anyhow::anyhow!(
                    "frame processor `{}` bypassed a frame in strict mode",
                    node_snapshot.plugin_name
                ),
                decoder_frame.clone(),
            ));
        }
        Ok(())
    }

    fn handle_submit_rejected(
        &mut self,
        node_index: usize,
        submit_result: FrameProcessorSubmitResult,
        decoder_frame: &DecoderNativeFrame,
        state: &mut MacosFrameProcessorProcessState,
    ) -> Result<(), (anyhow::Error, DecoderNativeFrame)> {
        self.reset_to_decoder_frame(decoder_frame, state);
        self.debug.observe_bypass();
        state.debug_sample.bypassed = true;
        let node_snapshot = self.node_snapshot(node_index);
        self.push_warning(
            FrameProcessorWarningKind::Unsupported,
            &node_snapshot,
            &state.current_frame,
            FrameProcessorWarningDetails {
                queue_depth: submit_result.queue_depth,
                in_flight_frames: submit_result.in_flight_frames,
                ..FrameProcessorWarningDetails::default()
            },
            if self.mode == FrameProcessorMode::RequireProcessed {
                FrameProcessorPolicyAction::FailPlayback
            } else {
                FrameProcessorPolicyAction::BypassOriginalFrame
            },
            submit_result.message,
        );
        if self.mode == FrameProcessorMode::RequireProcessed {
            return Err((
                anyhow::anyhow!(
                    "frame processor `{}` rejected a frame in strict mode",
                    node_snapshot.plugin_name
                ),
                decoder_frame.clone(),
            ));
        }
        Ok(())
    }

    fn receive_from_node(
        &mut self,
        node_index: usize,
    ) -> anyhow::Result<FrameProcessorReceiveOutput> {
        let node = &mut self.processors[node_index];
        node.session.receive_frame().map_err(|error| {
            frame_processor_runtime_error(self.mode, node.processor_index, &node.plugin_name, error)
        })
    }

    fn handle_receive_output(
        &mut self,
        node_index: usize,
        receive_output: FrameProcessorReceiveOutput,
        decoder_frame: &DecoderNativeFrame,
        state: &mut MacosFrameProcessorProcessState,
    ) -> Result<(), (anyhow::Error, DecoderNativeFrame)> {
        match receive_output {
            FrameProcessorReceiveOutput::Frame(output) => {
                self.handle_ready_output(node_index, output, decoder_frame, state)
            }
            FrameProcessorReceiveOutput::Pending | FrameProcessorReceiveOutput::EndOfStream => {
                self.handle_pending_output(node_index, decoder_frame, state)
            }
        }
    }

    fn handle_ready_output(
        &mut self,
        node_index: usize,
        output: FrameProcessorOutputFrame,
        decoder_frame: &DecoderNativeFrame,
        state: &mut MacosFrameProcessorProcessState,
    ) -> Result<(), (anyhow::Error, DecoderNativeFrame)> {
        state.debug_sample.processed_nodes = state.debug_sample.processed_nodes.saturating_add(1);
        let node_snapshot = self.node_snapshot(node_index);
        let timing_decision =
            self.record_output_timing(&node_snapshot, &state.current_frame, &output);
        state.debug_sample.deadline_missed |= timing_decision.deadline_missed;
        state.debug_sample.dropped_output |= timing_decision.should_drop_output;
        if timing_decision.should_drop_output || timing_decision.should_fail_playback {
            self.release_processor_outputs(vec![ProcessorOwnedNativeFrame {
                processor_index: node_snapshot.processor_index,
                frame: output.frame.clone(),
            }]);
        }
        if timing_decision.should_fail_playback && self.mode == FrameProcessorMode::RequireProcessed
        {
            self.release_processor_outputs(std::mem::take(&mut state.processor_outputs));
            return Err((
                anyhow::anyhow!(
                    "frame processor `{}` missed frame deadline in strict mode",
                    node_snapshot.plugin_name
                ),
                decoder_frame.clone(),
            ));
        }
        if timing_decision.should_drop_output {
            self.reset_to_decoder_frame(decoder_frame, state);
            return Ok(());
        }
        self.accept_processor_output(output.frame, &node_snapshot, decoder_frame, state);
        Ok(())
    }

    fn accept_processor_output(
        &mut self,
        output_frame: NativeFrame,
        node_snapshot: &MacosFrameProcessorNodeSnapshot,
        decoder_frame: &DecoderNativeFrame,
        state: &mut MacosFrameProcessorProcessState,
    ) {
        if output_frame_requires_processor_release(&output_frame) {
            state.processor_outputs.push(ProcessorOwnedNativeFrame {
                processor_index: node_snapshot.processor_index,
                frame: output_frame.clone(),
            });
        }
        state.current_frame = output_frame;
        if self.mode == FrameProcessorMode::DiagnosticsOnly {
            state.current_frame = decoder_frame_to_native_frame(decoder_frame);
            state.using_processor_output = false;
        } else {
            state.using_processor_output = true;
        }
    }

    fn handle_pending_output(
        &mut self,
        node_index: usize,
        decoder_frame: &DecoderNativeFrame,
        state: &mut MacosFrameProcessorProcessState,
    ) -> Result<(), (anyhow::Error, DecoderNativeFrame)> {
        self.reset_to_decoder_frame(decoder_frame, state);
        self.metrics.bypassed_frame_count = self.metrics.bypassed_frame_count.saturating_add(1);
        self.debug.observe_bypass();
        self.debug.observe_pending();
        state.debug_sample.bypassed = true;
        state.debug_sample.pending = true;
        let node_snapshot = self.node_snapshot(node_index);
        self.push_warning(
            FrameProcessorWarningKind::BypassActivated,
            &node_snapshot,
            &state.current_frame,
            FrameProcessorWarningDetails::default(),
            FrameProcessorPolicyAction::BypassOriginalFrame,
            Some("processor did not return a ready frame".to_owned()),
        );
        if self.mode == FrameProcessorMode::RequireProcessed {
            return Err((
                anyhow::anyhow!(
                    "frame processor `{}` did not return a ready frame in strict mode",
                    node_snapshot.plugin_name
                ),
                decoder_frame.clone(),
            ));
        }
        Ok(())
    }

    fn reset_to_decoder_frame(
        &mut self,
        decoder_frame: &DecoderNativeFrame,
        state: &mut MacosFrameProcessorProcessState,
    ) {
        self.release_processor_outputs(std::mem::take(&mut state.processor_outputs));
        state.current_frame = decoder_frame_to_native_frame(decoder_frame);
        state.using_processor_output = false;
    }

    fn finish_process_state(
        &mut self,
        decoder_frame: DecoderNativeFrame,
        mut state: MacosFrameProcessorProcessState,
    ) -> MacosFrameProcessorFrame {
        let presentation_frame = if self.mode == FrameProcessorMode::PreferProcessed
            || self.mode == FrameProcessorMode::RequireProcessed
        {
            native_frame_to_decoder_frame(&state.current_frame)
        } else {
            decoder_frame.clone()
        };
        state.debug_sample.output_pts_us = presentation_frame.metadata.pts_us;
        state.debug_sample.presented_processed = state.using_processor_output;
        self.debug.finish_frame(state.debug_sample);
        MacosFrameProcessorFrame {
            decoder_frame,
            presentation_frame,
            processor_outputs: state.processor_outputs,
        }
    }

    fn record_output_timing(
        &mut self,
        node: &MacosFrameProcessorNodeSnapshot,
        input: &NativeFrame,
        output: &FrameProcessorOutputFrame,
    ) -> MacosFrameProcessorTimingDecision {
        self.metrics.processed_frame_count = self.metrics.processed_frame_count.saturating_add(1);
        self.metrics.last_queue_wait_us = output.timings.queue_wait_us;
        self.metrics.last_process_time_us = output.timings.process_time_us;
        self.metrics.last_submit_to_ready_us = output.timings.submit_to_ready_us;
        let mut decision = MacosFrameProcessorTimingDecision::default();
        if output
            .timings
            .submit_to_ready_us
            .is_some_and(|elapsed| elapsed > self.policy.frame_deadline.as_micros() as u64)
        {
            self.metrics.deadline_miss_count = self.metrics.deadline_miss_count.saturating_add(1);
            self.debug.observe_deadline_miss();
            decision.deadline_missed = true;
            let action = if self.mode == FrameProcessorMode::RequireProcessed {
                FrameProcessorPolicyAction::FailPlayback
            } else {
                FrameProcessorPolicyAction::BypassOriginalFrame
            };
            self.push_warning(
                FrameProcessorWarningKind::DeadlineMissed,
                node,
                input,
                FrameProcessorWarningDetails::from_output_timing(
                    output,
                    self.policy.frame_deadline,
                ),
                action,
                Some("processor output missed frame deadline".to_owned()),
            );
            if self.mode == FrameProcessorMode::RequireProcessed {
                decision.should_fail_playback = true;
            }
        }
        if output.timings.submit_to_ready_us.is_some_and(|elapsed| {
            elapsed
                > (self.policy.frame_deadline + self.policy.late_output_tolerance).as_micros()
                    as u64
        }) {
            decision.should_drop_output = true;
            self.metrics.dropped_output_count = self.metrics.dropped_output_count.saturating_add(1);
            self.metrics.late_output_drop_count =
                self.metrics.late_output_drop_count.saturating_add(1);
            self.debug.observe_dropped_output();
            self.push_warning(
                FrameProcessorWarningKind::LateOutputDropped,
                node,
                input,
                FrameProcessorWarningDetails::from_output_timing(
                    output,
                    self.policy.frame_deadline,
                ),
                FrameProcessorPolicyAction::DropOutput,
                Some("processor output was later than tolerance".to_owned()),
            );
        }
        decision
    }

    fn release_processor_outputs(&mut self, mut outputs: Vec<ProcessorOwnedNativeFrame>) {
        while let Some(output) = outputs.pop() {
            if let Some(node) = self
                .processors
                .iter_mut()
                .find(|node| node.processor_index == output.processor_index)
            {
                let _ = node.session.release_frame(output.frame);
            }
        }
    }

    fn drain_events(&mut self) -> Vec<PlayerRuntimeEvent> {
        self.pending_events.drain(..).collect()
    }

    fn flush(&mut self) {
        for node in &mut self.processors {
            let _ = node.session.flush();
        }
    }

    fn push_warning(
        &mut self,
        kind: FrameProcessorWarningKind,
        node: &MacosFrameProcessorNodeSnapshot,
        input: &NativeFrame,
        details: FrameProcessorWarningDetails,
        policy_action: FrameProcessorPolicyAction,
        message: Option<String>,
    ) {
        self.pending_events.push_back(PlayerRuntimeEvent::Warning(
            PlayerRuntimeWarning::FrameProcessor(FrameProcessorWarning {
                kind,
                plugin_name: node.plugin_name.clone(),
                processor_index: node.processor_index,
                frame_id: input.metadata.frame_id,
                frame_pts_us: input.metadata.pts_us,
                frame_duration_us: input.metadata.duration_us,
                input_handle_kind: Some(format!("{:?}", input.metadata.handle_kind)),
                output_handle_kind: details.output_handle_kind,
                queue_depth: details.queue_depth,
                in_flight_frames: details.in_flight_frames,
                queue_wait_us: details.queue_wait_us.or(self.metrics.last_queue_wait_us),
                process_time_us: details
                    .process_time_us
                    .or(self.metrics.last_process_time_us),
                submit_to_ready_us: details
                    .submit_to_ready_us
                    .or(self.metrics.last_submit_to_ready_us),
                present_deadline_us: input
                    .metadata
                    .pts_us
                    .map(|pts| pts.saturating_add(duration_us_i64(self.policy.frame_deadline))),
                deadline_overrun_us: details.deadline_overrun_us,
                consecutive_miss_count: None,
                policy_action,
                message,
            }),
        ));
    }

    fn node_snapshot(&self, node_index: usize) -> MacosFrameProcessorNodeSnapshot {
        let node = &self.processors[node_index];
        MacosFrameProcessorNodeSnapshot {
            plugin_name: node.plugin_name.clone(),
            processor_index: node.processor_index,
        }
    }
}

#[derive(Debug, Clone)]
struct MacosFrameProcessorNodeSnapshot {
    plugin_name: String,
    processor_index: usize,
}

#[derive(Debug, Default)]
struct FrameProcessorWarningDetails {
    output_handle_kind: Option<String>,
    queue_depth: Option<u32>,
    in_flight_frames: Option<u32>,
    queue_wait_us: Option<u64>,
    process_time_us: Option<u64>,
    submit_to_ready_us: Option<u64>,
    deadline_overrun_us: Option<u64>,
}

impl FrameProcessorWarningDetails {
    fn from_output_timing(output: &FrameProcessorOutputFrame, deadline: Duration) -> Self {
        let deadline_us = deadline.as_micros() as u64;
        Self {
            output_handle_kind: Some(format!("{:?}", output.frame.metadata.handle_kind)),
            queue_wait_us: output.timings.queue_wait_us,
            process_time_us: output.timings.process_time_us,
            submit_to_ready_us: output.timings.submit_to_ready_us,
            deadline_overrun_us: output
                .timings
                .submit_to_ready_us
                .and_then(|elapsed| elapsed.checked_sub(deadline_us)),
            ..Self::default()
        }
    }
}

#[derive(Debug, Default)]
struct MacosFrameProcessorTimingDecision {
    should_drop_output: bool,
    should_fail_playback: bool,
    deadline_missed: bool,
}

fn frame_processor_runtime_error(
    mode: FrameProcessorMode,
    processor_index: usize,
    plugin_name: &str,
    error: FrameProcessorError,
) -> anyhow::Error {
    if mode == FrameProcessorMode::RequireProcessed {
        anyhow::anyhow!(
            "frame processor `{plugin_name}` at index {processor_index} failed in strict mode: {error}"
        )
    } else {
        anyhow::anyhow!(
            "frame processor `{plugin_name}` at index {processor_index} failed: {error}"
        )
    }
}

fn duration_us_i64(duration: Duration) -> i64 {
    i64::try_from(duration.as_micros()).unwrap_or(i64::MAX)
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
}

fn max_option_u32(current: Option<u32>, next: Option<u32>) -> Option<u32> {
    current.max(next)
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
    let requirements = match record.capability_summary.as_ref() {
        Some(PluginCapabilitySummary::Decoder(capabilities)) => {
            capabilities.native_requirements.as_ref()
        }
        _ => None,
    };
    if requirements.is_some_and(|requirements| {
        requirements.requires_native_device_context
            || (!requirements.output_handle_kinds.is_empty()
                && !requirements
                    .output_handle_kinds
                    .contains(&DecoderNativeHandleKind::CvPixelBuffer))
    }) {
        return None;
    }
    Some(MacosNativeFrameDecoderSelection {
        plugin_path: record.path.clone(),
        plugin_name: record.plugin_name.clone(),
        video_surface,
        frame_processor_paths: if options.frame_processor_mode == FrameProcessorMode::Disabled {
            Vec::new()
        } else {
            options.frame_processor_library_paths.clone()
        },
        frame_processor_mode: options.frame_processor_mode,
        frame_processor_policy: options.frame_processor_policy.clone(),
    })
}

fn select_macos_source_normalizer_packet_decoder(
    stream_info: Option<&player_plugin::SourceNormalizerPacketStreamInfo>,
    options: &PlayerRuntimeOptions,
) -> Option<MacosNativeFrameDecoderSelection> {
    if options.decoder_plugin_video_mode != PlayerDecoderPluginVideoMode::PreferNativeFrame {
        return None;
    }
    let video_surface = options.video_surface?;
    if options.decoder_plugin_library_paths.is_empty() {
        return None;
    }
    let video_stream = macos_packet_stream_info_from_source_normalizer(stream_info?).ok()?;
    if video_stream.codec.is_empty() {
        return None;
    }
    let request = DecoderPluginMatchRequest::video(video_stream.codec);
    let registry = PluginRegistry::inspect_decoder_support(
        &options.decoder_plugin_library_paths,
        request.clone(),
    );
    let record = registry.best_native_decoder_for(&request)?;
    let requirements = match record.capability_summary.as_ref() {
        Some(PluginCapabilitySummary::Decoder(capabilities)) => {
            capabilities.native_requirements.as_ref()
        }
        _ => None,
    };
    if requirements.is_some_and(|requirements| {
        requirements.requires_native_device_context
            || (!requirements.output_handle_kinds.is_empty()
                && !requirements
                    .output_handle_kinds
                    .contains(&DecoderNativeHandleKind::CvPixelBuffer))
    }) {
        return None;
    }
    Some(MacosNativeFrameDecoderSelection {
        plugin_path: record.path.clone(),
        plugin_name: record.plugin_name.clone(),
        video_surface,
        frame_processor_paths: if options.frame_processor_mode == FrameProcessorMode::Disabled {
            Vec::new()
        } else {
            options.frame_processor_library_paths.clone()
        },
        frame_processor_mode: options.frame_processor_mode,
        frame_processor_policy: options.frame_processor_policy.clone(),
    })
}

fn source_normalizer_packet_decoder_unavailable_message(
    normalization: &MacosSourceNormalizationOutcome,
    options: &PlayerRuntimeOptions,
) -> Option<String> {
    let stream_info = normalization.packet_stream_info.as_ref()?;
    let video_stream = macos_packet_stream_info_from_source_normalizer(stream_info).ok()?;
    if options.decoder_plugin_video_mode != PlayerDecoderPluginVideoMode::PreferNativeFrame {
        return Some(format!(
            "source normalizer packet stream for {} video requires native-frame decoder plugin mode",
            video_stream.codec
        ));
    }
    if options.video_surface.is_none() {
        return Some(format!(
            "source normalizer packet stream for {} video requires a macOS video surface",
            video_stream.codec
        ));
    }
    if options.decoder_plugin_library_paths.is_empty() {
        return Some(format!(
            "source normalizer packet stream for {} video requires a decoder plugin path",
            video_stream.codec
        ));
    }
    let request = DecoderPluginMatchRequest::video(video_stream.codec.clone());
    let registry = PluginRegistry::inspect_decoder_support(
        &options.decoder_plugin_library_paths,
        request.clone(),
    );
    Some(format!(
        "source normalizer packet stream for {} video found no matching native-frame decoder plugin: {}",
        video_stream.codec,
        source_normalizer_registry_notes(&registry)
    ))
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

fn macos_decoder_bitstream_format(codec: &str) -> DecoderBitstreamFormat {
    match codec.to_ascii_uppercase().as_str() {
        "HEVC" | "H265" | "HVC1" | "HEV1" => DecoderBitstreamFormat::Hvcc,
        _ => DecoderBitstreamFormat::Avcc,
    }
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
    if let Some(registry) = frame_processor_plugin_registry(options) {
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
    let startup = apply_video_decode_diagnostics(startup, &diagnostics.video_decode);
    append_plugin_diagnostics(startup, &diagnostics.plugin_diagnostics)
}

fn append_plugin_diagnostics(
    mut startup: PlayerRuntimeStartup,
    diagnostics: &[PlayerPluginDiagnostic],
) -> PlayerRuntimeStartup {
    for diagnostic in diagnostics {
        if startup
            .plugin_diagnostics
            .iter()
            .any(|existing| same_plugin_diagnostic(existing, diagnostic))
        {
            continue;
        }
        startup.plugin_diagnostics.push(diagnostic.clone());
    }
    startup
}

fn same_plugin_diagnostic(left: &PlayerPluginDiagnostic, right: &PlayerPluginDiagnostic) -> bool {
    left.path == right.path
        && left.plugin_name == right.plugin_name
        && left.plugin_kind == right.plugin_kind
        && left.status == right.status
        && left.message == right.message
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
    if let Some(registry) = decoder_plugin_registry(media_info, options) {
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
    }
    apply_frame_processor_plugin_diagnostics(startup, options)
}

fn apply_frame_processor_plugin_diagnostics(
    mut startup: PlayerRuntimeStartup,
    options: &PlayerRuntimeOptions,
) -> PlayerRuntimeStartup {
    let Some(registry) = frame_processor_plugin_registry(options) else {
        return startup;
    };
    startup.plugin_diagnostics.extend(
        registry
            .records()
            .iter()
            .map(player_plugin_diagnostic_from_record),
    );
    startup
}

fn prepare_source_normalizer_for_open(
    source: MediaSource,
    options: &PlayerRuntimeOptions,
) -> PlayerResult<MacosSourceNormalizationOutcome> {
    let mut outcome = MacosSourceNormalizationOutcome {
        source: source.clone(),
        packet_session: None,
        packet_stream_info: None,
        diagnostics: Vec::new(),
        selected_profile: None,
        normalized_endpoint: None,
        ready_latency: None,
    };
    if options.source_normalizer_mode == SourceNormalizerMode::Disabled {
        return Ok(outcome);
    }

    if should_bypass_source_normalizer_for_native_adaptive(&source) {
        let protocol = match source.protocol() {
            MediaSourceProtocol::Hls => "HLS",
            MediaSourceProtocol::Dash => "DASH",
            _ => "adaptive",
        };
        outcome
            .diagnostics
            .push(source_normalizer_runtime_diagnostic(
                None,
                format!(
                    "source normalizer packet stream skipped for {protocol} adaptive source; selected native adaptive playback path"
                ),
            ));
        return Ok(outcome);
    }

    if options.source_normalizer_plugin_library_paths.is_empty() {
        let message =
            "source normalizer requested but no source normalizer plugin paths are configured"
                .to_owned();
        outcome
            .diagnostics
            .push(source_normalizer_runtime_diagnostic(None, message.clone()));
        return match options.source_normalizer_mode {
            SourceNormalizerMode::RequireNormalized => Err(PlayerError::new(
                PlayerErrorCode::Unsupported,
                format!("{message}; source normalizer mode is RequireNormalized"),
            )),
            SourceNormalizerMode::Disabled | SourceNormalizerMode::PreferNormalized => Ok(outcome),
        };
    }

    let registry = PluginRegistry::inspect_source_normalizer_support(
        &options.source_normalizer_plugin_library_paths,
    );
    outcome.diagnostics.extend(
        registry
            .records()
            .iter()
            .map(player_plugin_diagnostic_from_record),
    );
    if registry.best_source_normalizer().is_none() {
        let message = format!(
            "source normalizer requested but no supported source normalizer plugin is available: {}",
            source_normalizer_registry_notes(&registry)
        );
        outcome
            .diagnostics
            .push(source_normalizer_runtime_diagnostic(None, message.clone()));
        return match options.source_normalizer_mode {
            SourceNormalizerMode::RequireNormalized => {
                Err(PlayerError::new(PlayerErrorCode::Unsupported, message))
            }
            SourceNormalizerMode::Disabled | SourceNormalizerMode::PreferNormalized => Ok(outcome),
        };
    }

    if let Some(packet_record) = registry.best_source_normalizer_packet() {
        match open_source_normalizer_packet_session(&source, options, packet_record) {
            Ok(ready) => {
                outcome.selected_profile = ready.selected_profile.clone();
                outcome.ready_latency = Some(ready.ready_latency);
                outcome.normalized_endpoint =
                    ready.stream_info.session_id.as_ref().map(|session_id| {
                        format!("vesper-source-normalizer-packet://{session_id}")
                    });
                outcome.packet_stream_info = Some(ready.stream_info);
                outcome.packet_session = Some(ready.session);
                outcome
                    .diagnostics
                    .push(source_normalizer_runtime_diagnostic(
                        ready.plugin_name.clone(),
                        format!(
                            "source normalizer selected profile {} via {}; ready in {} ms; output packet_stream",
                            ready.selected_profile.as_deref().unwrap_or("auto-detected"),
                            ready.plugin_name.as_deref().unwrap_or("unknown-normalizer"),
                            ready.ready_latency.as_millis()
                        ),
                    ));
                return Ok(outcome);
            }
            Err(error) => {
                let message =
                    format!("source normalizer packet stream failed before playback: {error}");
                outcome
                    .diagnostics
                    .push(source_normalizer_runtime_diagnostic(
                        packet_record.plugin_name.clone(),
                        message.clone(),
                    ));
                if options.source_normalizer_mode == SourceNormalizerMode::RequireNormalized {
                    return Err(PlayerError::new(PlayerErrorCode::BackendFailure, message));
                }
            }
        }
    } else {
        let message = format!(
            "source normalizer requested but no source_normalizer_packet_v2 plugin is available: {}",
            source_normalizer_registry_notes(&registry)
        );
        outcome
            .diagnostics
            .push(source_normalizer_runtime_diagnostic(None, message.clone()));
        if options.source_normalizer_mode == SourceNormalizerMode::RequireNormalized {
            return Err(PlayerError::new(PlayerErrorCode::Unsupported, message));
        }
    }

    if options.source_normalizer_mode == SourceNormalizerMode::RequireNormalized {
        let message = "source normalizer mode is RequireNormalized but no normalized packet stream was produced".to_owned();
        outcome
            .diagnostics
            .push(source_normalizer_runtime_diagnostic(None, message.clone()));
        return Err(PlayerError::new(PlayerErrorCode::BackendFailure, message));
    }

    Ok(outcome)
}

fn should_bypass_source_normalizer_for_native_adaptive(source: &MediaSource) -> bool {
    matches!(
        source.protocol(),
        MediaSourceProtocol::Hls | MediaSourceProtocol::Dash
    )
}

struct ReadySourceNormalizerPacketSession {
    session: Arc<Mutex<Option<Box<dyn SourceNormalizerPacketSession>>>>,
    stream_info: player_plugin::SourceNormalizerPacketStreamInfo,
    selected_profile: Option<String>,
    plugin_name: Option<String>,
    ready_latency: Duration,
}

fn open_source_normalizer_packet_session(
    source: &MediaSource,
    _options: &PlayerRuntimeOptions,
    record: &PluginDiagnosticRecord,
) -> Result<ReadySourceNormalizerPacketSession, String> {
    let plugin = LoadedDynamicPlugin::load(&record.path)
        .map_err(|error| format!("failed to load source normalizer plugin: {error}"))?;
    let factory = plugin
        .source_normalizer_packet_plugin_factory()
        .ok_or_else(|| {
            format!(
                "{} is not a packet source normalizer plugin",
                plugin.plugin_name()
            )
        })?;
    let config = SourceNormalizerPacketSessionConfig {
        runtime_profile: String::new(),
        input: source.uri().to_owned(),
        headers: Vec::new(),
        startup_timeout_ms: Some(SOURCE_NORMALIZER_STARTUP_TIMEOUT.as_millis() as u64),
        session_timeout_ms: Some(SOURCE_NORMALIZER_SESSION_TIMEOUT.as_millis() as u64),
        preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
    };
    let started = Instant::now();
    let session = factory
        .open_packet_session(&config)
        .map_err(|error| format!("open_packet_session failed: {error}"))?;
    let stream_info = session.stream_info();
    macos_packet_stream_info_from_source_normalizer(&stream_info)
        .map_err(|error| format!("invalid packet stream info: {error}"))?;
    Ok(ReadySourceNormalizerPacketSession {
        selected_profile: stream_info.runtime_profile.clone(),
        plugin_name: stream_info
            .normalizer_name
            .clone()
            .or_else(|| Some(factory.name().to_owned())),
        ready_latency: started.elapsed(),
        stream_info,
        session: Arc::new(Mutex::new(Some(session))),
    })
}

fn macos_packet_stream_info_from_source_normalizer(
    stream_info: &player_plugin::SourceNormalizerPacketStreamInfo,
) -> anyhow::Result<VideoPacketStreamInfo> {
    let track = stream_info
        .selected_track_index
        .and_then(|selected| {
            stream_info
                .tracks
                .iter()
                .find(|track| track.stream_index == selected)
        })
        .or_else(|| {
            stream_info
                .tracks
                .iter()
                .find(|track| track.media_kind == SourceNormalizerPacketMediaKind::Video)
        })
        .ok_or_else(|| anyhow::anyhow!("source normalizer packet stream has no video track"))?;
    macos_packet_track_info_from_source_normalizer(track)
}

fn macos_packet_track_info_from_source_normalizer(
    track: &SourceNormalizerPacketTrackInfo,
) -> anyhow::Result<VideoPacketStreamInfo> {
    if track.media_kind != SourceNormalizerPacketMediaKind::Video {
        anyhow::bail!("selected source normalizer packet track is not video");
    }
    Ok(VideoPacketStreamInfo {
        stream_index: usize::try_from(track.stream_index).unwrap_or(usize::MAX),
        codec: track.codec.clone(),
        extradata: track.extradata.clone(),
        width: track.width,
        height: track.height,
        frame_rate: track.frame_rate,
    })
}

fn source_normalizer_runtime_diagnostic(
    plugin_name: Option<String>,
    message: String,
) -> PlayerPluginDiagnostic {
    PlayerPluginDiagnostic {
        path: String::new(),
        plugin_name,
        plugin_kind: Some("source_normalizer".to_owned()),
        status: PlayerPluginDiagnosticStatus::Loaded,
        message: Some(message),
        capability: None,
    }
}

fn source_normalizer_registry_notes(registry: &PluginRegistry) -> String {
    let notes = registry
        .records()
        .iter()
        .map(PluginDiagnosticRecord::summary)
        .collect::<Vec<_>>();
    if notes.is_empty() {
        "no plugin paths were inspected".to_owned()
    } else {
        notes.join("; ")
    }
}

fn apply_source_normalizer_open_diagnostics(
    mut startup: PlayerRuntimeStartup,
    normalization: &MacosSourceNormalizationOutcome,
) -> PlayerRuntimeStartup {
    for diagnostic in &normalization.diagnostics {
        startup.plugin_diagnostics.push(diagnostic.clone());
    }
    startup
}

fn drop_source_normalizer_packet_session(normalization: &mut MacosSourceNormalizationOutcome) {
    if let Some(packet_session) = normalization.packet_session.take()
        && let Ok(mut guard) = packet_session.lock()
        && let Some(mut session) = guard.take()
    {
        let _ = session.close();
    }
    normalization.packet_stream_info = None;
}

fn attach_source_normalizer_to_runtime(
    bootstrap: PlayerRuntimeBootstrap,
    mut normalization: MacosSourceNormalizationOutcome,
) -> PlayerRuntimeBootstrap {
    if normalization.packet_session.is_some() {
        let packet_session = normalization.packet_session.take();
        let adapter_id = bootstrap.runtime.adapter_id().to_owned();
        let PlayerRuntimeBootstrap {
            runtime,
            initial_frame,
            startup,
        } = bootstrap;
        let source_normalizer_diagnostics = startup
            .plugin_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.plugin_kind.as_deref() == Some("source_normalizer"))
            .cloned()
            .collect::<Vec<_>>();
        let adapter_id = if adapter_id == MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID {
            MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID
        } else if adapter_id == MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID {
            MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
        } else {
            MACOS_HOST_PLAYER_RUNTIME_ADAPTER_ID
        };
        return PlayerRuntime::from_adapter_bootstrap(
            adapter_id,
            PlayerRuntimeAdapterBootstrap {
                runtime: Box::new(MacosSourceNormalizerRuntimeGuard {
                    inner: runtime,
                    source_normalizer_packet_session: packet_session,
                    source_normalizer_diagnostics,
                }),
                initial_frame,
                startup,
            },
        );
    }
    bootstrap
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

fn frame_processor_plugin_registry(options: &PlayerRuntimeOptions) -> Option<PluginRegistry> {
    if options.frame_processor_mode == FrameProcessorMode::Disabled
        || options.frame_processor_library_paths.is_empty()
    {
        return None;
    }
    Some(PluginRegistry::inspect_frame_processor_support(
        &options.frame_processor_library_paths,
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
            if matches!(
                record.capability_summary.as_ref(),
                Some(PluginCapabilitySummary::Decoder(capabilities))
                    if capabilities.supports_native_frame_output
            ) {
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
            PluginDiagnosticStatus::FrameProcessorSupported => {
                PlayerPluginDiagnosticStatus::FrameProcessorSupported
            }
            PluginDiagnosticStatus::FrameProcessorUnsupported => {
                PlayerPluginDiagnosticStatus::FrameProcessorUnsupported
            }
            PluginDiagnosticStatus::SourceNormalizerSupported => {
                PlayerPluginDiagnosticStatus::Loaded
            }
            PluginDiagnosticStatus::SourceNormalizerUnsupported => {
                PlayerPluginDiagnosticStatus::UnsupportedKind
            }
        },
        message: record.message.clone(),
        capability: record
            .capability_summary
            .as_ref()
            .and_then(player_plugin_capability_summary_from_loader),
    }
}

fn player_plugin_capability_summary_from_loader(
    summary: &PluginCapabilitySummary,
) -> Option<PlayerPluginCapabilitySummary> {
    match summary {
        PluginCapabilitySummary::Decoder(summary) => Some(PlayerPluginCapabilitySummary::Decoder(
            player_decoder_capability_summary_from_loader(summary),
        )),
        PluginCapabilitySummary::FrameProcessor(summary) => {
            Some(PlayerPluginCapabilitySummary::FrameProcessor(
                player_frame_processor_capability_summary_from_loader(summary),
            ))
        }
        PluginCapabilitySummary::SourceNormalizerPacket(_) => None,
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

fn player_frame_processor_capability_summary_from_loader(
    summary: &FrameProcessorPluginCapabilitySummary,
) -> PlayerPluginFrameProcessorCapabilitySummary {
    PlayerPluginFrameProcessorCapabilitySummary {
        accepted_input_handle_kinds: summary
            .accepted_input_handle_kinds
            .iter()
            .map(native_handle_kind_label)
            .collect(),
        output_handle_kinds: summary
            .output_handle_kinds
            .iter()
            .map(native_handle_kind_label)
            .collect(),
        supports_video_frames: summary.supports_video_frames,
        supports_in_place_passthrough: summary.supports_in_place_passthrough,
        preserves_dimensions: summary.preserves_dimensions,
        may_change_dimensions: summary.may_change_dimensions,
        preserves_color_metadata: summary.preserves_color_metadata,
        preserves_hdr_metadata: summary.preserves_hdr_metadata,
        supports_flush: summary.supports_flush,
        max_sessions: summary.max_sessions,
        max_in_flight_frames: summary.max_in_flight_frames,
    }
}

fn native_handle_kind_label(handle_kind: &NativeHandleKind) -> String {
    match handle_kind {
        NativeHandleKind::CvPixelBuffer => "cv_pixel_buffer".to_owned(),
        NativeHandleKind::IoSurface => "io_surface".to_owned(),
        NativeHandleKind::MetalTexture => "metal_texture".to_owned(),
        NativeHandleKind::DmaBuf => "dma_buf".to_owned(),
        NativeHandleKind::VaapiSurface => "vaapi_surface".to_owned(),
        NativeHandleKind::D3D11Texture2D => "d3d11_texture_2d".to_owned(),
        NativeHandleKind::DxgiSurface => "dxgi_surface".to_owned(),
        NativeHandleKind::VulkanImage => "vulkan_image".to_owned(),
        NativeHandleKind::Unknown(name) => name.clone(),
    }
}

fn plugin_kind_label(kind: VesperPluginKind) -> &'static str {
    match kind {
        VesperPluginKind::PostDownloadProcessor => "post_download_processor",
        VesperPluginKind::PipelineEventHook => "pipeline_event_hook",
        VesperPluginKind::Decoder => "decoder",
        VesperPluginKind::BenchmarkSink => "benchmark_sink",
        VesperPluginKind::FrameProcessor => "frame_processor",
        VesperPluginKind::SourceNormalizer => "source_normalizer",
    }
}

fn should_prefer_native_host_runtime(
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
) -> bool {
    if should_route_macos_host_to_decoder_plugin_path(media_info, options) {
        return false;
    }
    options.video_surface.is_some() || media_info.best_video.is_none()
}

fn should_route_macos_host_to_decoder_plugin_path(
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
) -> bool {
    media_info.best_video.is_some()
        && options.decoder_plugin_video_mode == PlayerDecoderPluginVideoMode::PreferNativeFrame
}

fn macos_host_software_path_reason(
    media_info: &PlayerMediaInfo,
    options: &PlayerRuntimeOptions,
) -> Option<String> {
    let best_video = media_info.best_video.as_ref()?;
    if should_route_macos_host_to_decoder_plugin_path(media_info, options) {
        return Some(format!(
            "native-frame decoder plugin playback requested for {} video; selected desktop decoder plugin path",
            best_video.codec
        ));
    }
    Some(format!(
        "macos native host runtime requires an external video surface for {} playback; selected software desktop path",
        best_video.codec
    ))
}

fn probe_macos_host_runtime_initializer_with_factories(
    source: MediaSource,
    options: PlayerRuntimeOptions,
    native_factory: &dyn PlayerRuntimeAdapterFactory,
    software_fallback_factory: Arc<dyn MacosHostFallbackFactory>,
) -> PlayerResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
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
                let fallback_reason = macos_host_software_path_reason(&media_info, &options);
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
) -> PlayerResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
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
    normalization: MacosSourceNormalizationOutcome,
) -> PlayerResult<PlayerRuntimeBootstrap> {
    let forward_strict_frame_processor_error = strict_frame_processor_fallback_enabled(&options);
    let open_options = without_source_normalizer_options(options);
    match PlayerRuntime::open_source_with_factory(
        source,
        open_options,
        macos_runtime_adapter_factory(),
    ) {
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
            bootstrap.startup =
                apply_source_normalizer_open_diagnostics(bootstrap.startup, &normalization);
            Ok(attach_source_normalizer_to_runtime(
                bootstrap,
                normalization,
            ))
        }
        Err(software_error) => match fallback_reason {
            Some(fallback_reason) => {
                if should_forward_strict_frame_processor_fallback_error(
                    forward_strict_frame_processor_error,
                    &software_error,
                ) {
                    return Err(software_error);
                }
                Err(PlayerError::new(
                    PlayerErrorCode::BackendFailure,
                    format!(
                        "macos native host playback failed and software fallback also failed: native={}, software={}",
                        fallback_reason,
                        software_error.message()
                    ),
                ))
            }
            None => Err(software_error),
        },
    }
}

fn open_software_fallback_runtime_with_interrupt(
    source: MediaSource,
    options: PlayerRuntimeOptions,
    interrupt_flag: Arc<AtomicBool>,
    fallback_reason: Option<String>,
    normalization: MacosSourceNormalizationOutcome,
) -> PlayerResult<PlayerRuntimeBootstrap> {
    let forward_strict_frame_processor_error = strict_frame_processor_fallback_enabled(&options);
    let open_options = without_source_normalizer_options(options);
    match open_macos_software_runtime_source_with_options_and_interrupt(
        source,
        open_options,
        interrupt_flag,
    ) {
        Ok(mut bootstrap) => {
            apply_video_decode_fallback_reason(&mut bootstrap.startup, fallback_reason);
            bootstrap.startup =
                apply_source_normalizer_open_diagnostics(bootstrap.startup, &normalization);
            Ok(attach_source_normalizer_to_runtime(
                bootstrap,
                normalization,
            ))
        }
        Err(software_error) => match fallback_reason {
            Some(fallback_reason) => {
                if should_forward_strict_frame_processor_fallback_error(
                    forward_strict_frame_processor_error,
                    &software_error,
                ) {
                    return Err(software_error);
                }
                Err(PlayerError::new(
                    PlayerErrorCode::BackendFailure,
                    format!(
                        "macos native host playback failed and software fallback also failed: native={}, software={}",
                        fallback_reason,
                        software_error.message()
                    ),
                ))
            }
            None => Err(software_error),
        },
    }
}

fn open_software_fallback_adapter_with_factory(
    source: MediaSource,
    options: PlayerRuntimeOptions,
    software_factory: &dyn MacosHostFallbackFactory,
    fallback_reason: Option<String>,
) -> PlayerResult<PlayerRuntimeAdapterBootstrap> {
    let forward_strict_frame_processor_error = strict_frame_processor_fallback_enabled(&options);
    let initializer = software_factory.probe_source_with_options(source, options)?;
    let mut startup = initializer.startup();
    apply_video_decode_fallback_reason(&mut startup, fallback_reason);
    let mut bootstrap = match initializer.initialize() {
        Ok(bootstrap) => bootstrap,
        Err(software_error)
            if should_forward_strict_frame_processor_fallback_error(
                forward_strict_frame_processor_error,
                &software_error,
            ) =>
        {
            return Err(software_error);
        }
        Err(software_error) => return Err(software_error),
    };
    bootstrap.startup = startup;
    Ok(bootstrap)
}

fn strict_frame_processor_fallback_enabled(options: &PlayerRuntimeOptions) -> bool {
    options.frame_processor_mode == FrameProcessorMode::RequireProcessed
        && !options.frame_processor_library_paths.is_empty()
}

fn without_source_normalizer_options(mut options: PlayerRuntimeOptions) -> PlayerRuntimeOptions {
    options.source_normalizer_mode = SourceNormalizerMode::Disabled;
    options.source_normalizer_plugin_library_paths.clear();
    options
}

fn should_forward_strict_frame_processor_fallback_error(
    strict_frame_processor_fallback: bool,
    error: &PlayerError,
) -> bool {
    strict_frame_processor_fallback
        && error.code() == PlayerErrorCode::BackendFailure
        && error
            .message()
            .contains("frame processor initialization failed in strict mode")
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    #[cfg(target_os = "macos")]
    use std::os::raw::c_void;
    use std::path::{Path, PathBuf};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc,
    };
    use std::time::{Duration, Instant};

    #[cfg(target_os = "macos")]
    use super::macos_runtime_adapter_factory;
    use super::{
        FrameProcessorDebugState, MACOS_HOST_PLAYER_RUNTIME_ADAPTER_ID,
        MACOS_NATIVE_PLAYER_RUNTIME_ADAPTER_ID, MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
        MacosFrameProcessorChain, MacosFrameProcessorNode, MacosHostPlayerRuntimeAdapterFactory,
        MacosNativeFrameDecoderState, MacosNativeFramePacketSendStatus,
        MacosNativeFramePacketSource, MacosNativeFramePrefetchWakeup, MacosNativeFrameVideoSource,
        MacosRuntimeActiveFallback, MacosRuntimeAdapter, MacosRuntimeAdapterFallback,
        MacosRuntimeAdapterInitializer, MacosRuntimeDiagnostics,
        MacosSoftwarePlayerRuntimeAdapterFactory, MacosSourceNormalizationOutcome,
        apply_decoder_plugin_diagnostics, apply_decoder_plugin_diagnostics_to_video_decode,
        apply_decoder_plugin_registry_to_video_decode, apply_source_normalizer_open_diagnostics,
        attach_source_normalizer_to_runtime, macos_native_frame_decoder_video_decode_info,
        macos_runtime_diagnostics, macos_video_decode_info,
        open_macos_host_runtime_source_with_options,
        open_macos_software_runtime_source_with_options_and_interrupt,
        prepare_source_normalizer_for_open, present_and_release_native_frame_with_presenter,
        present_if_current_epoch_and_release, probe_macos_host_runtime_initializer_with_factories,
        probe_macos_host_runtime_source_with_options, process_macos_native_frame,
        release_native_frame_with_counter, send_macos_native_frame_packet,
        should_forward_strict_frame_processor_fallback_error,
        should_trigger_runtime_fallback_for_advance, should_trigger_runtime_fallback_for_command,
        source_normalizer_packet_decoder_unavailable_message,
        spawn_macos_native_frame_prefetch_worker, strict_frame_processor_fallback_enabled,
        without_source_normalizer_options,
    };
    use player_backend_ffmpeg::{
        CompressedVideoPacket, FfmpegBackend, VideoPacketSource, VideoPacketStreamInfo,
    };
    use player_model::MediaSource;
    use player_platform_apple::VIDEOTOOLBOX_BACKEND_NAME;
    use player_platform_desktop::{DesktopVideoFramePoll, DesktopVideoSource};
    use player_plugin::{
        DecoderBitstreamFormat, DecoderError, DecoderMediaKind, DecoderNativeFrame,
        DecoderNativeFrameMetadata, DecoderNativeHandleKind, DecoderPacket, DecoderPacketResult,
        DecoderReceiveNativeFrameOutput, DecoderSessionConfig, DecoderSessionInfo,
        FrameProcessorError, FrameProcessorFrameTimings, FrameProcessorOutputFrame,
        FrameProcessorReceiveOutput, FrameProcessorSession, FrameProcessorSessionInfo,
        FrameProcessorSubmitFrame, FrameProcessorSubmitResult, FrameProcessorSubmitStatus,
        NativeDecoderSession, NativeFrame, SourceNormalizerError, SourceNormalizerOperationStatus,
        SourceNormalizerPacket, SourceNormalizerPacketLease, SourceNormalizerPacketMediaKind,
        SourceNormalizerPacketSeek, SourceNormalizerPacketSession,
        SourceNormalizerPacketStreamInfo, SourceNormalizerPacketTrackInfo,
        SourceNormalizerReadPacketMetadata, VesperPluginKind,
    };
    use player_plugin_loader::{
        DecoderPluginCapabilitySummary, DecoderPluginCodecSummary, LoadedDynamicPlugin,
        PluginCapabilitySummary, PluginDiagnosticRecord, PluginDiagnosticStatus, PluginRegistry,
    };
    use player_runtime::{
        DecodedVideoFrame, FrameProcessorMode, FrameProcessorPolicy, FrameProcessorPolicyAction,
        FrameProcessorWarningKind, PlaybackProgress, PlayerError, PlayerErrorCode,
        PlayerFrameProcessingMetrics, PlayerMediaInfo, PlayerPluginCapabilitySummary,
        PlayerPluginDiagnostic, PlayerPluginDiagnosticStatus, PlayerResult, PlayerRuntime,
        PlayerRuntimeAdapter, PlayerRuntimeAdapterBackendFamily, PlayerRuntimeAdapterBootstrap,
        PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory,
        PlayerRuntimeAdapterInitializer, PlayerRuntimeCommand, PlayerRuntimeCommandResult,
        PlayerRuntimeEvent, PlayerRuntimeOptions, PlayerRuntimeStartup, PlayerRuntimeWarning,
        PlayerVideoDecodeInfo, PlayerVideoDecodeMode, PlayerVideoInfo, PlayerVideoSurfaceKind,
        PlayerVideoSurfaceTarget, PresentationState, SourceNormalizerMode,
    };
    #[cfg(target_os = "macos")]
    use player_runtime::{PlayerDecoderPluginVideoMode, PlayerRuntimeInitializer};

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
                eprintln!(
                    "skipping macOS fixture-backed test: fixtures/media/tiny-h264-aac.m4v is unavailable"
                );
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
            assert_eq!(error.code(), PlayerErrorCode::Unsupported);
        }
    }

    #[test]
    fn macos_host_factory_without_surface_prefers_software_path() {
        if !cfg!(target_os = "macos") {
            return;
        }

        let Some(test_video_path) = test_video_path() else {
            eprintln!(
                "skipping macOS fixture-backed test: fixtures/media/tiny-h264-aac.m4v is unavailable"
            );
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
            eprintln!(
                "skipping macOS fixture-backed test: fixtures/media/tiny-h264-aac.m4v is unavailable"
            );
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
    fn macos_host_strategy_routes_explicit_native_frame_request_to_plugin_path() {
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
            initialize_error: None,
            advance_error: None,
        };
        let software_factory = FakeStrategyFactory {
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
            initialize_error: None,
            advance_error: None,
        };
        unsafe {
            std::env::set_var("VESPER_MACOS_TEST_FORCE_PRESENTER_FAILURE", "1");
        }
        let options = PlayerRuntimeOptions::default()
            .with_video_surface(PlayerVideoSurfaceTarget {
                kind: PlayerVideoSurfaceKind::PlayerLayer,
                handle: 0x1234,
            })
            .with_decoder_plugin_video_mode(PlayerDecoderPluginVideoMode::PreferNativeFrame);

        let initializer = probe_macos_host_runtime_initializer_with_factories(
            MediaSource::new("fixture.mp4"),
            options,
            &native_factory,
            Arc::new(software_factory),
        )
        .expect("host strategy probe should route to desktop plugin path");

        assert_eq!(
            initializer.capabilities().backend_family,
            PlayerRuntimeAdapterBackendFamily::SoftwareDesktop
        );
        assert_eq!(
            initializer.capabilities().adapter_id,
            MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
        );
        assert!(
            initializer
                .startup()
                .video_decode
                .as_ref()
                .and_then(|info| info.fallback_reason.as_deref())
                .unwrap_or_default()
                .contains("selected desktop decoder plugin path")
        );
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
            initialize_error: Some(PlayerError::new(
                PlayerErrorCode::BackendFailure,
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
    fn strict_frame_processor_fallback_error_is_forwarded_without_host_wrapper() {
        let mut strict_options = PlayerRuntimeOptions::default()
            .with_frame_processor_library_paths([PathBuf::from("fixture-frame-processor")])
            .with_frame_processor_mode(FrameProcessorMode::RequireProcessed);
        assert!(strict_frame_processor_fallback_enabled(&strict_options));
        let strict_error = PlayerError::new(
            PlayerErrorCode::BackendFailure,
            "native-frame frame processor initialization failed in strict mode: unsupported native handle kind: CvPixelBuffer",
        );

        assert!(should_forward_strict_frame_processor_fallback_error(
            strict_frame_processor_fallback_enabled(&strict_options),
            &strict_error
        ));

        strict_options.frame_processor_mode = FrameProcessorMode::PreferProcessed;
        assert!(!should_forward_strict_frame_processor_fallback_error(
            strict_frame_processor_fallback_enabled(&strict_options),
            &strict_error
        ));
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
            initialize_error: Some(PlayerError::new(
                PlayerErrorCode::BackendFailure,
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
            strict_frame_processor_error_prefix: None,
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
    fn software_runtime_initializer_returns_native_frame_error_without_fallback() {
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
            initialize_error: Some(PlayerError::new(
                PlayerErrorCode::BackendFailure,
                "native-frame init failed",
            )),
            advance_error: None,
        });
        let diagnostics = MacosRuntimeDiagnostics {
            video_decode: macos_native_frame_decoder_video_decode_info(Some("fixture-native")),
            plugin_diagnostics: Vec::new(),
            has_video_surface: true,
        };
        let initializer = Box::new(MacosRuntimeAdapterInitializer {
            inner: native_inner,
            diagnostics,
            fallback: None,
            runtime_fallback: None,
            strict_frame_processor_error_prefix: Some(
                "native-frame frame processor initialization failed in strict mode".to_owned(),
            ),
        });

        let error = match initializer.initialize() {
            Ok(_) => panic!("strict native-frame initializer should not fall back"),
            Err(error) => error,
        };

        assert_eq!(error.code(), PlayerErrorCode::BackendFailure);
        assert!(
            error
                .message()
                .contains("frame processor initialization failed in strict mode")
        );
        assert!(error.message().contains("native-frame init failed"));
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
            advance_error: Some(PlayerError::new(
                PlayerErrorCode::BackendFailure,
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
            plugin_diagnostics: Vec::new(),
            has_video_surface: true,
            runtime_fallback: Some(MacosRuntimeActiveFallback {
                source: fallback_source.clone(),
                options: fallback_options.clone(),
                fallback_reason:
                    "native-frame runtime failed during playback; selected FFmpeg software path"
                        .to_owned(),
            }),
            pending_runtime_fallback_events: VecDeque::new(),
            source_normalizer_packet_session: None,
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
            dispatch_error: Some(PlayerError::new(
                PlayerErrorCode::BackendFailure,
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
            plugin_diagnostics: Vec::new(),
            has_video_surface: true,
            runtime_fallback: Some(MacosRuntimeActiveFallback {
                source: MediaSource::new("fixture.mp4"),
                options: PlayerRuntimeOptions::default(),
                fallback_reason:
                    "native-frame runtime failed during playback; selected FFmpeg software path"
                        .to_owned(),
            }),
            pending_runtime_fallback_events: VecDeque::new(),
            source_normalizer_packet_session: None,
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
                    dispatch_error: Some(PlayerError::new(
                        PlayerErrorCode::BackendFailure,
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
                plugin_diagnostics: Vec::new(),
                has_video_surface: true,
                runtime_fallback: Some(MacosRuntimeActiveFallback {
                    source: MediaSource::new("fixture.mp4"),
                    options: PlayerRuntimeOptions::default(),
                    fallback_reason:
                        "native-frame runtime failed during playback; selected FFmpeg software path"
                            .to_owned(),
                }),
                pending_runtime_fallback_events: VecDeque::new(),
                source_normalizer_packet_session: None,
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
            &PlayerError::new(
                PlayerErrorCode::BackendFailure,
                "failed to present decoded video frame"
            )
        ));
        assert!(should_trigger_runtime_fallback_for_advance(
            &PlayerError::new(
                PlayerErrorCode::BackendFailure,
                "failed to present seeked video frame"
            )
        ));
        assert!(!should_trigger_runtime_fallback_for_advance(
            &PlayerError::new(
                PlayerErrorCode::BackendFailure,
                "failed to decode audio stream"
            )
        ));
        assert!(should_trigger_runtime_fallback_for_advance(
            &PlayerError::new(
                PlayerErrorCode::BackendFailure,
                "native-frame decoder state is poisoned"
            )
        ));
        assert!(!should_trigger_runtime_fallback_for_advance(
            &PlayerError::new(
                PlayerErrorCode::SeekFailure,
                "failed to present decoded video frame"
            )
        ));
        assert!(should_trigger_runtime_fallback_for_command(
            &PlayerRuntimeCommand::SeekTo {
                position: Duration::from_secs(1)
            },
            &PlayerError::new(PlayerErrorCode::BackendFailure, "forced seek failure")
        ));
        assert!(should_trigger_runtime_fallback_for_command(
            &PlayerRuntimeCommand::Play,
            &PlayerError::new(PlayerErrorCode::BackendFailure, "forced play failure")
        ));
        assert!(should_trigger_runtime_fallback_for_command(
            &PlayerRuntimeCommand::SetPlaybackRate { rate: 1.5 },
            &PlayerError::new(PlayerErrorCode::BackendFailure, "forced rate failure")
        ));
        assert!(!should_trigger_runtime_fallback_for_command(
            &PlayerRuntimeCommand::Pause,
            &PlayerError::new(PlayerErrorCode::BackendFailure, "forced pause failure")
        ));
        assert!(!should_trigger_runtime_fallback_for_command(
            &PlayerRuntimeCommand::Stop,
            &PlayerError::new(PlayerErrorCode::BackendFailure, "forced stop failure")
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
                    dispatch_error: Some(PlayerError::new(
                        PlayerErrorCode::BackendFailure,
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
                plugin_diagnostics: Vec::new(),
                has_video_surface: true,
                runtime_fallback: Some(MacosRuntimeActiveFallback {
                    source: MediaSource::new("fixture.mp4"),
                    options: PlayerRuntimeOptions::default(),
                    fallback_reason:
                        "native-frame runtime failed during playback; selected FFmpeg software path"
                            .to_owned(),
                }),
                pending_runtime_fallback_events: VecDeque::new(),
                source_normalizer_packet_session: None,
            };

            let error = adapter
                .dispatch(command)
                .expect_err("pause/stop should not fallback");
            assert_eq!(error.code(), PlayerErrorCode::BackendFailure);
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
    fn macos_source_normalizer_disabled_keeps_original_source() {
        let original = MediaSource::new("file:///tmp/original.flv");
        let outcome = prepare_source_normalizer_for_open(
            original.clone(),
            &PlayerRuntimeOptions::default().with_source_normalizer_plugin_library_paths([
                PathBuf::from("/tmp/missing-source-normalizer"),
            ]),
        )
        .expect("disabled source normalizer should not inspect plugin paths");

        assert_eq!(outcome.source.uri(), original.uri());
        assert!(outcome.packet_session.is_none());
        assert!(outcome.diagnostics.is_empty());
    }

    #[test]
    fn macos_source_normalizer_prefer_missing_plugin_falls_back_with_diagnostics() {
        let outcome = prepare_source_normalizer_for_open(
            MediaSource::new("file:///tmp/original.flv"),
            &PlayerRuntimeOptions::default()
                .with_source_normalizer_plugin_library_paths([PathBuf::from(
                    "/tmp/missing-source-normalizer",
                )])
                .with_source_normalizer_mode(SourceNormalizerMode::PreferNormalized),
        )
        .expect("prefer mode should fall back when a plugin is missing");

        assert_eq!(outcome.source.uri(), "file:///tmp/original.flv");
        assert!(outcome.packet_session.is_none());
        assert!(outcome.diagnostics.iter().any(|diagnostic| {
            diagnostic.status == PlayerPluginDiagnosticStatus::LoadFailed
                && diagnostic
                    .message
                    .as_deref()
                    .unwrap_or_default()
                    .contains("failed to open plugin library")
        }));
    }

    #[test]
    fn macos_source_normalizer_skips_native_adaptive_sources() {
        let original = MediaSource::new("https://example.test/live/master.m3u8");
        let outcome = prepare_source_normalizer_for_open(
            original.clone(),
            &PlayerRuntimeOptions::default()
                .with_source_normalizer_plugin_library_paths([PathBuf::from(
                    "/tmp/missing-source-normalizer",
                )])
                .with_source_normalizer_mode(SourceNormalizerMode::RequireNormalized),
        )
        .expect("native adaptive sources should bypass packet source normalization");

        assert_eq!(outcome.source.uri(), original.uri());
        assert!(outcome.packet_session.is_none());
        assert!(outcome.packet_stream_info.is_none());
        assert_eq!(outcome.diagnostics.len(), 1);
        assert!(
            outcome.diagnostics[0]
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("skipped for HLS adaptive source")
        );
    }

    #[test]
    fn macos_source_normalizer_require_missing_plugin_fails() {
        let result = prepare_source_normalizer_for_open(
            MediaSource::new("file:///tmp/original.flv"),
            &PlayerRuntimeOptions::default()
                .with_source_normalizer_plugin_library_paths([PathBuf::from(
                    "/tmp/missing-source-normalizer",
                )])
                .with_source_normalizer_mode(SourceNormalizerMode::RequireNormalized),
        );
        let error = match result {
            Ok(_) => panic!("require mode should fail when no plugin is available"),
            Err(error) => error,
        };

        assert_eq!(error.code(), PlayerErrorCode::Unsupported);
        assert!(
            error
                .message()
                .contains("no supported source normalizer plugin")
        );
    }

    #[test]
    fn macos_source_normalizer_diagnostics_are_attached_once_opened() {
        let normalization = MacosSourceNormalizationOutcome {
            source: MediaSource::new("/tmp/normalized.mp4"),
            packet_session: None,
            packet_stream_info: None,
            diagnostics: vec![PlayerPluginDiagnostic {
                path: String::new(),
                plugin_name: Some("fixture-normalizer".to_owned()),
                plugin_kind: Some("source_normalizer".to_owned()),
                status: PlayerPluginDiagnosticStatus::Loaded,
                message: Some("source normalizer selected profile fixture".to_owned()),
                capability: None,
            }],
            selected_profile: Some("fixture".to_owned()),
            normalized_endpoint: Some("/tmp/normalized.mp4".to_owned()),
            ready_latency: Some(Duration::from_millis(7)),
        };
        let startup = apply_source_normalizer_open_diagnostics(
            startup_with_video_decode(macos_video_decode_info(&media_info_with_codec("H264"))),
            &normalization,
        );

        assert!(startup.plugin_diagnostics.iter().any(|diagnostic| {
            diagnostic.plugin_kind.as_deref() == Some("source_normalizer")
                && diagnostic
                    .message
                    .as_deref()
                    .unwrap_or_default()
                    .contains("selected profile")
        }));
    }

    #[test]
    fn macos_source_normalizer_session_guard_keeps_runtime_source() {
        let stream_info = fake_source_normalizer_packet_stream_info("H264");
        let bootstrap = PlayerRuntime::from_adapter_bootstrap(
            MACOS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
            PlayerRuntimeAdapterBootstrap {
                runtime: Box::new(FakeStrategyRuntime {
                    capabilities: default_software_capabilities(),
                    media_info: media_info_with_source_uri("/tmp/normalized.mp4", "H264"),
                    playback_rate: 1.0,
                    progress: PlaybackProgress::new(Duration::ZERO, None),
                    state: PresentationState::Ready,
                    events: VecDeque::new(),
                    advance_error: None,
                    dispatch_error: None,
                }),
                initial_frame: None,
                startup: startup_with_video_decode(macos_video_decode_info(
                    &media_info_with_codec("H264"),
                )),
            },
        );
        let bootstrap = attach_source_normalizer_to_runtime(
            bootstrap,
            MacosSourceNormalizationOutcome {
                source: MediaSource::new("/tmp/normalized.mp4"),
                packet_session: Some(Arc::new(Mutex::new(Some(Box::new(
                    FakeSourceNormalizerPacketSession::new(stream_info),
                ))))),
                packet_stream_info: None,
                diagnostics: Vec::new(),
                selected_profile: Some("fixture".to_owned()),
                normalized_endpoint: Some("/tmp/normalized.mp4".to_owned()),
                ready_latency: Some(Duration::from_millis(1)),
            },
        );

        assert_eq!(bootstrap.runtime.source_uri(), "/tmp/normalized.mp4");
    }

    #[test]
    fn macos_source_normalizer_packet_decoder_requires_strict_decoder_inputs() {
        let stream_info = fake_source_normalizer_packet_stream_info("H264");
        let normalization = MacosSourceNormalizationOutcome {
            source: MediaSource::new("file:///tmp/original.mp4"),
            packet_session: Some(Arc::new(Mutex::new(Some(Box::new(
                FakeSourceNormalizerPacketSession::new(stream_info.clone()),
            ))))),
            packet_stream_info: Some(stream_info),
            diagnostics: Vec::new(),
            selected_profile: Some("fixture-packet".to_owned()),
            normalized_endpoint: Some("vesper-source-normalizer-packet://fake-session".to_owned()),
            ready_latency: Some(Duration::from_millis(1)),
        };

        let message = source_normalizer_packet_decoder_unavailable_message(
            &normalization,
            &PlayerRuntimeOptions::default()
                .with_source_normalizer_mode(SourceNormalizerMode::RequireNormalized),
        )
        .expect("missing decoder mode should produce diagnostics");

        assert!(message.contains("requires native-frame decoder plugin mode"));
    }

    #[test]
    fn macos_source_normalizer_options_are_cleared_for_fallback_reopen() {
        let options = PlayerRuntimeOptions::default()
            .with_source_normalizer_plugin_library_paths([PathBuf::from("plugin")])
            .with_source_normalizer_mode(SourceNormalizerMode::PreferNormalized);
        let cleared = without_source_normalizer_options(options);

        assert_eq!(
            cleared.source_normalizer_mode,
            SourceNormalizerMode::Disabled
        );
        assert!(cleared.source_normalizer_plugin_library_paths.is_empty());
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
            assert!(matches!(
                diagnostic.capability.as_ref(),
                Some(PlayerPluginCapabilitySummary::Decoder(capabilities))
                    if capabilities.supports_native_frame_output
            ));
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
        let mut accepted_packets = 0usize;
        let mut decoded_frames = 0usize;
        let mut decoded_pts = Vec::new();
        while submitted_packets < 120 {
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
            accepted_packets += 1;

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
                        decoded_pts.push(frame.metadata.pts_us);
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
        assert!(
            decoded_frames >= accepted_packets.saturating_sub(2),
            "VideoToolbox output was sparse for {source}: decoded {decoded_frames} frames from {accepted_packets} accepted packets; pts={decoded_pts:?}"
        );
        assert!(
            decoded_pts
                .iter()
                .flatten()
                .any(|pts| *pts > 0 && *pts < 1_000_000),
            "VideoToolbox output did not include non-keyframe-era PTS values from the first second: pts={decoded_pts:?}"
        );
    }

    #[test]
    #[ignore = "requires a built player-decoder-videotoolbox shared library and a local H264/HEVC source"]
    fn macos_videotoolbox_decoder_flush_seek_and_eof_headless() {
        if !cfg!(target_os = "macos") {
            return;
        }
        let Some(plugin_path) =
            std::env::var_os("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH").map(PathBuf::from)
        else {
            eprintln!(
                "skipping VideoToolbox lifecycle test: VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH is not set"
            );
            return;
        };
        if !plugin_path.is_file() {
            eprintln!(
                "skipping VideoToolbox lifecycle test: plugin path is missing: {}",
                plugin_path.display()
            );
            return;
        }
        let Some(source) = videotoolbox_smoke_source_path() else {
            eprintln!(
                "skipping VideoToolbox lifecycle test: no local H264/HEVC smoke source found"
            );
            return;
        };

        let (mut packet_source, mut session) =
            open_videotoolbox_smoke_packet_source_and_session(&plugin_path, &source);
        assert!(
            decode_one_videotoolbox_frame(packet_source.as_mut(), session.as_mut(), 120),
            "VideoToolbox should decode a frame before flush/seek"
        );

        session.flush().expect("VideoToolbox flush should succeed");
        packet_source
            .seek_to(Duration::from_millis(0))
            .expect("packet source seek should succeed after flush");
        assert!(
            decode_one_videotoolbox_frame(packet_source.as_mut(), session.as_mut(), 120),
            "VideoToolbox should decode a frame after flush/seek"
        );

        drain_videotoolbox_session_to_eof(packet_source.as_mut(), session.as_mut())
            .expect("VideoToolbox should report EOF after packet drain");
        session.close().expect("VideoToolbox session should close");
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
    #[ignore = "requires built player-decoder-videotoolbox and player-frame-processor-diagnostic shared libraries plus a local H264/HEVC source"]
    #[cfg(target_os = "macos")]
    fn macos_native_frame_runtime_loads_frame_processor_diagnostic_plugin() {
        let Some(decoder_plugin_path) =
            std::env::var_os("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH").map(PathBuf::from)
        else {
            eprintln!(
                "skipping native-frame frame processor test: VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH is not set"
            );
            return;
        };
        if !decoder_plugin_path.is_file() {
            eprintln!(
                "skipping native-frame frame processor test: decoder plugin path is missing: {}",
                decoder_plugin_path.display()
            );
            return;
        }
        let Some(frame_processor_path) =
            std::env::var_os("VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH").map(PathBuf::from)
        else {
            eprintln!(
                "skipping native-frame frame processor test: VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH is not set"
            );
            return;
        };
        if !frame_processor_path.is_file() {
            eprintln!(
                "skipping native-frame frame processor test: frame processor plugin path is missing: {}",
                frame_processor_path.display()
            );
            return;
        }
        let Some(source) = videotoolbox_smoke_source_path() else {
            eprintln!(
                "skipping native-frame frame processor test: no local H264/HEVC smoke source found"
            );
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
            .with_decoder_plugin_library_paths([decoder_plugin_path])
            .with_decoder_plugin_video_mode(PlayerDecoderPluginVideoMode::PreferNativeFrame)
            .with_frame_processor_library_paths([frame_processor_path])
            .with_frame_processor_mode(FrameProcessorMode::PreferProcessed);
        let bootstrap =
            open_macos_host_runtime_source_with_options(MediaSource::new(source), options)
                .expect("macOS host runtime should open the native-frame frame processor path");
        unsafe {
            std::env::remove_var("VESPER_MACOS_TEST_FORCE_PRESENTER_FAILURE");
        }

        assert!(
            bootstrap
                .runtime
                .capabilities()
                .supports_external_video_surface
        );
        assert!(
            bootstrap
                .startup
                .plugin_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.status
                    == PlayerPluginDiagnosticStatus::FrameProcessorSupported
                    && diagnostic.plugin_name.as_deref()
                        == Some("player-frame-processor-diagnostic")),
            "expected frame processor support diagnostic, got {:?}",
            bootstrap.startup.plugin_diagnostics
        );
        assert!(
            bootstrap
                .startup
                .video_decode
                .as_ref()
                .and_then(|info| info.fallback_reason.as_deref())
                .unwrap_or_default()
                .contains("selected for native-frame VideoToolbox playback"),
            "expected native-frame decoder selection diagnostic, got {:?}",
            bootstrap.startup.video_decode
        );

        unsafe {
            player_macos_test_release_object(layer_handle);
        }
    }

    #[test]
    #[ignore = "requires built player-decoder-videotoolbox and player-frame-processor-diagnostic shared libraries plus a local H264/HEVC source"]
    #[cfg(target_os = "macos")]
    fn macos_native_frame_strict_frame_processor_failure_does_not_fallback_to_software() {
        let Some(decoder_plugin_path) =
            std::env::var_os("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH").map(PathBuf::from)
        else {
            eprintln!(
                "skipping strict frame processor fallback test: VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH is not set"
            );
            return;
        };
        if !decoder_plugin_path.is_file() {
            eprintln!(
                "skipping strict frame processor fallback test: decoder plugin path is missing: {}",
                decoder_plugin_path.display()
            );
            return;
        }
        let Some(frame_processor_path) =
            std::env::var_os("VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH").map(PathBuf::from)
        else {
            eprintln!(
                "skipping strict frame processor fallback test: VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH is not set"
            );
            return;
        };
        if !frame_processor_path.is_file() {
            eprintln!(
                "skipping strict frame processor fallback test: frame processor plugin path is missing: {}",
                frame_processor_path.display()
            );
            return;
        }
        let Some(source) = videotoolbox_smoke_source_path() else {
            eprintln!(
                "skipping strict frame processor fallback test: no local H264/HEVC smoke source found"
            );
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
            .with_decoder_plugin_library_paths([decoder_plugin_path])
            .with_decoder_plugin_video_mode(PlayerDecoderPluginVideoMode::PreferNativeFrame)
            .with_frame_processor_library_paths([frame_processor_path])
            .with_frame_processor_mode(FrameProcessorMode::RequireProcessed);
        let error = match open_macos_software_runtime_source_with_options_and_interrupt(
            MediaSource::new(source),
            options,
            Arc::new(AtomicBool::new(false)),
        ) {
            Ok(_) => panic!("strict frame processor initialization should not fall back"),
            Err(error) => error,
        };
        unsafe {
            player_macos_test_release_object(layer_handle);
        }

        assert_eq!(error.code(), PlayerErrorCode::BackendFailure);
        assert!(
            error
                .message()
                .contains("frame processor initialization failed in strict mode"),
            "unexpected strict frame processor error: {}",
            error
        );
    }

    #[test]
    #[ignore = "requires built player-decoder-videotoolbox and player-frame-processor-diagnostic shared libraries plus a local H264/HEVC source"]
    #[cfg(target_os = "macos")]
    fn macos_host_strict_frame_processor_failure_forwards_software_error_message() {
        let Some(decoder_plugin_path) =
            std::env::var_os("VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH").map(PathBuf::from)
        else {
            eprintln!(
                "skipping host strict frame processor error test: VESPER_DECODER_VIDEOTOOLBOX_PLUGIN_PATH is not set"
            );
            return;
        };
        if !decoder_plugin_path.is_file() {
            eprintln!(
                "skipping host strict frame processor error test: decoder plugin path is missing: {}",
                decoder_plugin_path.display()
            );
            return;
        }
        let Some(frame_processor_path) =
            std::env::var_os("VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH").map(PathBuf::from)
        else {
            eprintln!(
                "skipping host strict frame processor error test: VESPER_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH is not set"
            );
            return;
        };
        if !frame_processor_path.is_file() {
            eprintln!(
                "skipping host strict frame processor error test: frame processor plugin path is missing: {}",
                frame_processor_path.display()
            );
            return;
        }
        let Some(source) = videotoolbox_smoke_source_path() else {
            eprintln!(
                "skipping host strict frame processor error test: no local H264/HEVC smoke source found"
            );
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
            .with_decoder_plugin_library_paths([decoder_plugin_path])
            .with_decoder_plugin_video_mode(PlayerDecoderPluginVideoMode::PreferNativeFrame)
            .with_frame_processor_library_paths([frame_processor_path])
            .with_frame_processor_mode(FrameProcessorMode::RequireProcessed);
        let error =
            match open_macos_host_runtime_source_with_options(MediaSource::new(source), options) {
                Ok(_) => {
                    unsafe {
                        player_macos_test_release_object(layer_handle);
                    }
                    panic!("strict host frame processor initialization should fail");
                }
                Err(error) => error,
            };
        unsafe {
            player_macos_test_release_object(layer_handle);
        }

        assert_eq!(error.code(), PlayerErrorCode::BackendFailure);
        assert!(
            error
                .message()
                .contains("frame processor initialization failed in strict mode"),
            "unexpected strict frame processor error: {}",
            error
        );
        assert!(
            !error.message().contains("software fallback also failed"),
            "strict frame processor error should not be wrapped as a fallback failure: {}",
            error
        );
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
        if bootstrap.runtime.capabilities().supports_frame_output
            && !bootstrap
                .runtime
                .capabilities()
                .supports_external_video_surface
        {
            assert!(
                bootstrap
                    .startup
                    .video_decode
                    .as_ref()
                    .and_then(|info| info.fallback_reason.as_deref())
                    .unwrap_or_default()
                    .contains("native-frame decoder plugin initialization failed"),
                "expected initialization fallback diagnostics when native-frame open falls back before presenter failure"
            );
            unsafe {
                player_macos_test_release_object(layer_handle);
            }
            return;
        }
        let mut runtime = bootstrap.runtime;
        let initial_rate = runtime.playback_rate();

        unsafe {
            std::env::set_var("VESPER_MACOS_TEST_FORCE_PRESENTER_FAILURE", "1");
        }
        let _ = runtime
            .dispatch(PlayerRuntimeCommand::Play)
            .expect("play should succeed");
        let _ = runtime
            .dispatch(PlayerRuntimeCommand::SetPlaybackRate { rate: 1.25 })
            .expect("set playback rate should succeed before fallback");
        let _ = runtime
            .dispatch(PlayerRuntimeCommand::SeekTo {
                position: Duration::ZERO,
            })
            .expect("seek should trigger presenter failure fallback instead of failing");

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
        let events = runtime.drain_events();
        for event in &events {
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
            "expected native surface detachment event after fallback, got {events:?}"
        );
        assert!(
            saw_runtime_fallback_error,
            "expected explicit runtime fallback error event after fallback, got {events:?}"
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
            eprintln!(
                "skipping macOS fixture-backed test: fixtures/media/tiny-h264-aac.m4v is unavailable"
            );
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
            eprintln!(
                "skipping macOS fixture-backed test: fixtures/media/tiny-h264-aac.m4v is unavailable"
            );
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
            eprintln!(
                "skipping macOS fixture-backed test: fixtures/media/tiny-h264-aac.m4v is unavailable"
            );
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
            eprintln!(
                "skipping macOS fixture-backed test: fixtures/media/tiny-h264-aac.m4v is unavailable"
            );
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
                coded_width: Some(1920),
                coded_height: Some(1080),
                visible_rect: None,
                handle_kind: DecoderNativeHandleKind::CvPixelBuffer,
                frame_id: Some(7),
                release_tracking: None,
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
                coded_width: Some(1280),
                coded_height: Some(720),
                visible_rect: None,
                handle_kind: DecoderNativeHandleKind::CvPixelBuffer,
                frame_id: Some(11),
                release_tracking: None,
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
                coded_width: Some(640),
                coded_height: Some(360),
                visible_rect: None,
                handle_kind: DecoderNativeHandleKind::CvPixelBuffer,
                frame_id: Some(13),
                release_tracking: None,
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

    #[test]
    fn native_frame_source_seek_flushes_before_packet_seek_and_resets_eof() {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let session_state = RecordingNativeDecoderState::shared(events.clone());
        let packet_source = FakeNativeFramePacketSource::with_seek_packets(
            Vec::new(),
            vec![test_compressed_packet(250_000)],
            events.clone(),
        );
        let outstanding_frames = Arc::new(AtomicUsize::new(0));
        let mut source = native_frame_source_for_test(
            packet_source,
            session_state.clone(),
            outstanding_frames.clone(),
            true,
            true,
        );

        let frame = source
            .seek_to(Duration::from_millis(250))
            .expect("seek should succeed")
            .expect("seek should decode a frame");

        let events = events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default();
        assert!(
            contains_ordered_events(&events, &["flush", "packet_seek", "send_packet"]),
            "seek should flush before packet seek and first post-seek packet: {events:?}"
        );
        assert_eq!(
            session_state
                .lock()
                .map(|state| state.flush_count)
                .unwrap_or_default(),
            1
        );
        assert_eq!(frame.presentation_time, Duration::from_micros(250_000));
        drop(frame);
        assert_eq!(outstanding_frames.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn native_frame_source_sends_eof_once_and_keeps_terminal_eof() {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let session_state = RecordingNativeDecoderState::shared(events.clone());
        let packet_source =
            FakeNativeFramePacketSource::with_seek_packets(Vec::new(), Vec::new(), events);
        let mut source = native_frame_source_for_test(
            packet_source,
            session_state.clone(),
            Arc::new(AtomicUsize::new(0)),
            false,
            false,
        );

        assert!(
            source
                .recv_frame()
                .expect("first receive should succeed")
                .is_none()
        );
        assert!(matches!(
            source
                .try_recv_frame()
                .expect("second poll should stay terminal"),
            DesktopVideoFramePoll::EndOfStream
        ));

        let sent_packets = session_state
            .lock()
            .map(|state| state.sent_packets.clone())
            .unwrap_or_default();
        assert_eq!(
            sent_packets
                .iter()
                .filter(|packet| packet.end_of_stream)
                .count(),
            1
        );
    }

    #[test]
    fn native_frame_source_seek_after_eof_allows_packets_again() {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let session_state = RecordingNativeDecoderState::shared(events.clone());
        let packet_source = FakeNativeFramePacketSource::with_seek_packets(
            Vec::new(),
            vec![test_compressed_packet(500_000)],
            events.clone(),
        );
        let outstanding_frames = Arc::new(AtomicUsize::new(0));
        let mut source = native_frame_source_for_test(
            packet_source,
            session_state.clone(),
            outstanding_frames.clone(),
            false,
            false,
        );

        assert!(
            source
                .recv_frame()
                .expect("initial eof should succeed")
                .is_none()
        );
        let frame = source
            .seek_to(Duration::from_millis(500))
            .expect("seek after eof should succeed")
            .expect("seek after eof should decode a frame");

        let events = events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default();
        assert!(
            contains_ordered_events(
                &events,
                &["send_eos", "flush", "packet_seek", "send_packet"]
            ),
            "seek after EOF should flush and resume packets in order: {events:?}"
        );
        assert_eq!(frame.presentation_time, Duration::from_micros(500_000));
        drop(frame);
        assert_eq!(outstanding_frames.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn dropping_deferred_native_frame_releases_without_presenting() {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let session_state = RecordingNativeDecoderState::shared(events.clone());
        let packet_source = FakeNativeFramePacketSource::with_seek_packets(
            vec![test_compressed_packet(1_000)],
            Vec::new(),
            events,
        );
        let outstanding_frames = Arc::new(AtomicUsize::new(0));
        let mut source = native_frame_source_for_test(
            packet_source,
            session_state.clone(),
            outstanding_frames.clone(),
            false,
            false,
        );

        let frame = source
            .recv_frame()
            .expect("frame receive should succeed")
            .expect("expected a deferred native frame");
        assert_eq!(outstanding_frames.load(Ordering::SeqCst), 1);

        drop(frame);

        assert_eq!(outstanding_frames.load(Ordering::SeqCst), 0);
        assert_eq!(
            session_state
                .lock()
                .map(|state| state.released_handles)
                .unwrap_or_default(),
            1
        );
    }

    #[test]
    fn frame_processor_prefer_mode_uses_processed_frame_and_releases_output() {
        let state = Arc::new(std::sync::Mutex::new(RecordingFrameProcessorState {
            output_handle_offset: 1_000,
            ..RecordingFrameProcessorState::default()
        }));
        let mut chain = frame_processor_chain_for_test(
            FrameProcessorMode::PreferProcessed,
            vec![RecordingFrameProcessorSession::new(state.clone())],
        );

        let processed = chain
            .process(test_native_frame(10, Some(33_000)))
            .expect("processor chain should produce a frame");

        assert_eq!(processed.presentation_frame.handle, 1_010);
        assert_eq!(processed.decoder_frame.handle, 10);
        assert_eq!(processed.processor_outputs.len(), 1);
        assert_eq!(chain.metrics.submitted_frame_count, 1);
        assert_eq!(chain.metrics.processed_frame_count, 1);

        chain.release_processor_outputs(processed.processor_outputs);
        assert_eq!(
            state
                .lock()
                .map(|state| state.released_handles.clone())
                .unwrap_or_default(),
            vec![1_010]
        );
    }

    #[test]
    fn frame_processor_prefer_mode_accepts_in_place_passthrough_output() {
        let state = Arc::new(std::sync::Mutex::new(RecordingFrameProcessorState {
            output_handle_offset: 0,
            output_requires_release: Some(false),
            ..RecordingFrameProcessorState::default()
        }));
        let mut chain = frame_processor_chain_for_test(
            FrameProcessorMode::PreferProcessed,
            vec![RecordingFrameProcessorSession::new(state.clone())],
        );

        let processed = chain
            .process(test_native_frame(10, Some(33_000)))
            .expect("processor chain should accept in-place passthrough output");

        assert_eq!(processed.presentation_frame.handle, 10);
        assert!(processed.processor_outputs.is_empty());

        chain.release_processor_outputs(processed.processor_outputs);
        assert!(
            state
                .lock()
                .map(|state| state.released_handles.is_empty())
                .unwrap_or_default()
        );
    }

    #[test]
    fn frame_processor_late_output_is_dropped_and_warns() {
        let state = Arc::new(std::sync::Mutex::new(RecordingFrameProcessorState {
            output_handle_offset: 2_000,
            submit_to_ready_us: Some(25_000),
            ..RecordingFrameProcessorState::default()
        }));
        let mut chain = frame_processor_chain_for_test(
            FrameProcessorMode::PreferProcessed,
            vec![RecordingFrameProcessorSession::new(state.clone())],
        );

        let processed = chain
            .process(test_native_frame(11, Some(66_000)))
            .expect("late output should bypass instead of failing in prefer mode");

        assert_eq!(processed.presentation_frame.handle, 11);
        assert!(processed.processor_outputs.is_empty());
        assert_eq!(chain.metrics.deadline_miss_count, 1);
        assert_eq!(chain.metrics.late_output_drop_count, 1);
        assert_eq!(chain.metrics.dropped_output_count, 1);
        assert_eq!(
            state
                .lock()
                .map(|state| state.released_handles.clone())
                .unwrap_or_default(),
            vec![2_011]
        );

        let events = chain.drain_events();
        assert!(
            events.iter().any(|event| matches!(
                event,
                PlayerRuntimeEvent::Warning(PlayerRuntimeWarning::FrameProcessor(warning))
                    if warning.kind == FrameProcessorWarningKind::LateOutputDropped
                        && warning.policy_action == FrameProcessorPolicyAction::DropOutput
                        && warning.processor_index == 0
                        && warning.output_handle_kind.as_deref() == Some("CvPixelBuffer")
                        && warning.submit_to_ready_us == Some(25_000)
                        && warning.deadline_overrun_us == Some(9_000)
            )),
            "late output should emit a processor-indexed warning"
        );
    }

    #[test]
    fn frame_processor_diagnostics_mode_runs_processor_but_presents_original() {
        let state = Arc::new(std::sync::Mutex::new(RecordingFrameProcessorState {
            output_handle_offset: 4_000,
            ..RecordingFrameProcessorState::default()
        }));
        let mut chain = frame_processor_chain_for_test(
            FrameProcessorMode::DiagnosticsOnly,
            vec![RecordingFrameProcessorSession::new(state.clone())],
        );

        let processed = chain
            .process(test_native_frame(13, Some(120_000)))
            .expect("diagnostics mode should still run processor");

        assert_eq!(processed.presentation_frame.handle, 13);
        assert_eq!(processed.processor_outputs.len(), 1);
        assert_eq!(
            state
                .lock()
                .map(|state| state.submitted_handles.clone())
                .unwrap_or_default(),
            vec![13]
        );

        chain.release_processor_outputs(processed.processor_outputs);
        assert_eq!(
            state
                .lock()
                .map(|state| state.released_handles.clone())
                .unwrap_or_default(),
            vec![4_013]
        );
    }

    #[test]
    fn frame_processor_backpressure_bypasses_and_reports_queue_state() {
        let state = Arc::new(std::sync::Mutex::new(RecordingFrameProcessorState {
            submit_status: Some(FrameProcessorSubmitStatus::Backpressure),
            forced_queue_depth: Some(3),
            forced_in_flight_frames: Some(2),
            ..RecordingFrameProcessorState::default()
        }));
        let mut chain = frame_processor_chain_for_test(
            FrameProcessorMode::PreferProcessed,
            vec![RecordingFrameProcessorSession::new(state)],
        );

        let processed = chain
            .process(test_native_frame(14, Some(140_000)))
            .expect("backpressure should bypass original in prefer mode");

        assert_eq!(processed.presentation_frame.handle, 14);
        assert_eq!(chain.metrics.bypassed_frame_count, 1);
        assert_eq!(chain.metrics.backpressure_count, 1);
        let events = chain.drain_events();
        assert!(
            events.iter().any(|event| matches!(
                event,
                PlayerRuntimeEvent::Warning(PlayerRuntimeWarning::FrameProcessor(warning))
                    if warning.kind == FrameProcessorWarningKind::Backpressure
                        && warning.policy_action == FrameProcessorPolicyAction::BypassOriginalFrame
                        && warning.queue_depth == Some(3)
                        && warning.in_flight_frames == Some(2)
            )),
            "backpressure should carry queue and in-flight state"
        );
    }

    #[test]
    fn frame_processor_rejected_frame_fails_in_strict_mode() {
        let state = Arc::new(std::sync::Mutex::new(RecordingFrameProcessorState {
            submit_status: Some(FrameProcessorSubmitStatus::Rejected),
            ..RecordingFrameProcessorState::default()
        }));
        let mut chain = frame_processor_chain_for_test(
            FrameProcessorMode::RequireProcessed,
            vec![RecordingFrameProcessorSession::new(state)],
        );

        let error = chain
            .process(test_native_frame(15, Some(160_000)))
            .expect_err("strict mode should fail on rejected frame");

        assert!(error.0.to_string().contains("strict mode"));
        let events = chain.drain_events();
        assert!(
            events.iter().any(|event| matches!(
                event,
                PlayerRuntimeEvent::Warning(PlayerRuntimeWarning::FrameProcessor(warning))
                    if warning.kind == FrameProcessorWarningKind::Unsupported
                        && warning.policy_action == FrameProcessorPolicyAction::FailPlayback
                        && warning.processor_index == 0
            )),
            "strict rejected frame should emit unsupported warning before failing"
        );
    }

    #[test]
    fn frame_processor_strict_deadline_failure_releases_processor_and_decoder_frames() {
        let state = Arc::new(std::sync::Mutex::new(RecordingFrameProcessorState {
            output_handle_offset: 3_000,
            submit_to_ready_us: Some(17_000),
            ..RecordingFrameProcessorState::default()
        }));
        let mut shared = MacosNativeFrameDecoderState {
            frame_processor_chain: Some(frame_processor_chain_for_test(
                FrameProcessorMode::RequireProcessed,
                vec![RecordingFrameProcessorSession::new(state.clone())],
            )),
            presenter: None,
            presentation_epoch: 0,
        };

        let error = process_macos_native_frame(&mut shared, test_native_frame(12, Some(99_000)))
            .expect_err("strict mode should fail playback on deadline miss");

        assert!(error.0.to_string().contains("strict mode"));
        assert_eq!(
            state
                .lock()
                .map(|state| state.released_handles.clone())
                .unwrap_or_default(),
            vec![3_012]
        );
    }

    #[test]
    fn frame_processor_chain_flushes_sessions() {
        let first_state = Arc::new(std::sync::Mutex::new(
            RecordingFrameProcessorState::default(),
        ));
        let second_state = Arc::new(std::sync::Mutex::new(
            RecordingFrameProcessorState::default(),
        ));
        let mut chain = frame_processor_chain_for_test(
            FrameProcessorMode::DiagnosticsOnly,
            vec![
                RecordingFrameProcessorSession::new(first_state.clone()),
                RecordingFrameProcessorSession::new(second_state.clone()),
            ],
        );

        chain.flush();

        assert_eq!(
            first_state
                .lock()
                .map(|state| state.flush_count)
                .unwrap_or_default(),
            1
        );
        assert_eq!(
            second_state
                .lock()
                .map(|state| state.flush_count)
                .unwrap_or_default(),
            1
        );
    }

    fn media_info_with_codec(codec: &str) -> PlayerMediaInfo {
        media_info_with_source_uri("fixture.mp4", codec)
    }

    fn media_info_with_source_uri(source_uri: &str, codec: &str) -> PlayerMediaInfo {
        PlayerMediaInfo {
            source_uri: source_uri.to_owned(),
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

    fn default_software_capabilities() -> PlayerRuntimeAdapterCapabilities {
        PlayerRuntimeAdapterCapabilities {
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
        let decoder_capabilities = DecoderPluginCapabilitySummary {
            typed_codecs: vec![DecoderPluginCodecSummary {
                codec: codec.to_owned(),
                media_kind: DecoderMediaKind::Video,
            }],
            codecs: vec![format!("Video:{codec}")],
            supports_native_frame_output,
            native_requirements: None,
            supports_hardware_decode: false,
            supports_cpu_video_frames: !supports_native_frame_output,
            supports_audio_frames: false,
            supports_gpu_handles: supports_native_frame_output,
            supports_flush: true,
            supports_drain: true,
            max_sessions: Some(1),
        };
        PluginDiagnosticRecord {
            path: PathBuf::from("fixture-decoder"),
            status,
            plugin_name: Some("fixture-decoder".to_owned()),
            plugin_kind: Some(VesperPluginKind::Decoder),
            capability_summary: Some(PluginCapabilitySummary::Decoder(decoder_capabilities)),
            message: Some(message.to_owned()),
        }
    }

    fn test_video_path() -> Option<String> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../fixtures/media/tiny-h264-aac.m4v");
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

    fn open_videotoolbox_smoke_packet_source_and_session(
        plugin_path: &Path,
        source: &str,
    ) -> (Box<VideoPacketSource>, Box<dyn NativeDecoderSession>) {
        let backend = FfmpegBackend::new().expect("FFmpeg should initialize");
        let packet_source = backend
            .open_video_packet_source(MediaSource::new(source.to_owned()))
            .unwrap_or_else(|error| panic!("failed to open packet source `{source}`: {error}"));
        let stream_info = packet_source.stream_info().clone();
        let plugin = LoadedDynamicPlugin::load(plugin_path).unwrap_or_else(|error| {
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
            panic!(
                "VideoToolbox plugin does not support smoke source codec {}",
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
            .expect("VideoToolbox native session should open");
        (Box::new(packet_source), session)
    }

    fn decode_one_videotoolbox_frame(
        packet_source: &mut VideoPacketSource,
        session: &mut dyn NativeDecoderSession,
        max_packets: usize,
    ) -> bool {
        let mut submitted_packets = 0usize;
        while submitted_packets < max_packets {
            let Some(packet) = packet_source
                .next_packet()
                .expect("packet demux should succeed")
            else {
                return false;
            };
            submitted_packets = submitted_packets.saturating_add(1);
            let accepted = send_videotoolbox_packet(session, packet)
                .expect("VideoToolbox should accept compressed packet")
                .accepted;
            if !accepted {
                continue;
            }
            if receive_and_release_videotoolbox_frames(session).0 > 0 {
                return true;
            }
        }
        false
    }

    fn drain_videotoolbox_session_to_eof(
        packet_source: &mut VideoPacketSource,
        session: &mut dyn NativeDecoderSession,
    ) -> Result<(), &'static str> {
        while let Some(packet) = packet_source
            .next_packet()
            .expect("packet demux should succeed")
        {
            let _ = send_videotoolbox_packet(session, packet)
                .expect("VideoToolbox should accept compressed packet");
            if receive_and_release_videotoolbox_frames(session).1 {
                return Ok(());
            };
        }

        session
            .send_packet(
                &DecoderPacket {
                    end_of_stream: true,
                    ..DecoderPacket::default()
                },
                &[],
            )
            .expect("VideoToolbox should accept EOF packet");
        for _ in 0..16 {
            if receive_and_release_videotoolbox_frames(session).1 {
                return Ok(());
            }
        }
        Err("VideoToolbox did not emit EOF after end-of-stream packet")
    }

    fn send_videotoolbox_packet(
        session: &mut dyn NativeDecoderSession,
        packet: CompressedVideoPacket,
    ) -> Result<DecoderPacketResult, DecoderError> {
        session.send_packet(
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
    }

    fn receive_and_release_videotoolbox_frames(
        session: &mut dyn NativeDecoderSession,
    ) -> (usize, bool) {
        let mut decoded_frames = 0usize;
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
                    decoded_frames = decoded_frames.saturating_add(1);
                }
                DecoderReceiveNativeFrameOutput::NeedMoreInput => return (decoded_frames, false),
                DecoderReceiveNativeFrameOutput::Eof => return (decoded_frames, true),
            }
        }
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
        initialize_error: Option<PlayerError>,
        advance_error: Option<PlayerError>,
    }

    impl PlayerRuntimeAdapterFactory for FakeStrategyFactory {
        fn adapter_id(&self) -> &'static str {
            self.capabilities.adapter_id
        }

        fn probe_source_with_options(
            &self,
            _source: MediaSource,
            _options: PlayerRuntimeOptions,
        ) -> PlayerResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
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
        ) -> PlayerResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
            <Self as PlayerRuntimeAdapterFactory>::probe_source_with_options(self, source, options)
        }
    }

    struct FakeStrategyInitializer {
        capabilities: PlayerRuntimeAdapterCapabilities,
        media_info: PlayerMediaInfo,
        startup: PlayerRuntimeStartup,
        initialize_error: Option<PlayerError>,
        advance_error: Option<PlayerError>,
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

        fn initialize(self: Box<Self>) -> PlayerResult<PlayerRuntimeAdapterBootstrap> {
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
        advance_error: Option<PlayerError>,
        dispatch_error: Option<PlayerError>,
    }

    struct FakeSourceNormalizerPacketSession {
        stream_info: SourceNormalizerPacketStreamInfo,
        packet_data: Vec<u8>,
        emitted_packet: bool,
        outstanding_handle: Option<usize>,
        closed: bool,
    }

    impl FakeSourceNormalizerPacketSession {
        fn new(stream_info: SourceNormalizerPacketStreamInfo) -> Self {
            Self {
                stream_info,
                packet_data: vec![0, 0, 1, 9],
                emitted_packet: false,
                outstanding_handle: None,
                closed: false,
            }
        }
    }

    impl SourceNormalizerPacketSession for FakeSourceNormalizerPacketSession {
        fn stream_info(&self) -> SourceNormalizerPacketStreamInfo {
            self.stream_info.clone()
        }

        fn read_packet(
            &mut self,
        ) -> Result<SourceNormalizerPacketLease<'_>, SourceNormalizerError> {
            if self.closed {
                return Err(SourceNormalizerError::NotConfigured);
            }
            if self.outstanding_handle.is_some() {
                return Err(SourceNormalizerError::abi_violation(
                    "fake packet still needs release",
                ));
            }
            if self.emitted_packet {
                return Ok(SourceNormalizerPacketLease {
                    metadata: SourceNormalizerReadPacketMetadata::end_of_stream(),
                    data: &[],
                    handle: 0,
                });
            }
            self.emitted_packet = true;
            self.outstanding_handle = Some(1);
            Ok(SourceNormalizerPacketLease {
                metadata: SourceNormalizerReadPacketMetadata::packet(SourceNormalizerPacket {
                    pts_us: Some(0),
                    dts_us: Some(0),
                    duration_us: Some(41_667),
                    stream_index: 0,
                    key_frame: true,
                    discontinuity: false,
                    end_of_stream: false,
                }),
                data: &self.packet_data,
                handle: 1,
            })
        }

        fn release_packet(&mut self, packet_handle: usize) -> Result<(), SourceNormalizerError> {
            if self.outstanding_handle == Some(packet_handle) {
                self.outstanding_handle = None;
                Ok(())
            } else {
                Err(SourceNormalizerError::abi_violation(
                    "fake packet handle was not outstanding",
                ))
            }
        }

        fn seek(
            &mut self,
            _seek: &SourceNormalizerPacketSeek,
        ) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
            self.emitted_packet = false;
            self.outstanding_handle = None;
            Ok(SourceNormalizerOperationStatus {
                completed: true,
                message: None,
            })
        }

        fn flush(&mut self) -> Result<SourceNormalizerOperationStatus, SourceNormalizerError> {
            self.outstanding_handle = None;
            Ok(SourceNormalizerOperationStatus {
                completed: true,
                message: None,
            })
        }

        fn close(&mut self) -> Result<(), SourceNormalizerError> {
            self.closed = true;
            self.outstanding_handle = None;
            Ok(())
        }
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
        ) -> PlayerResult<PlayerRuntimeCommandResult> {
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

        fn advance(&mut self) -> PlayerResult<Option<DecodedVideoFrame>> {
            if let Some(error) = self.advance_error.take() {
                return Err(error);
            }
            Ok(None)
        }

        fn next_deadline(&self) -> Option<Instant> {
            None
        }
    }

    #[derive(Debug)]
    struct FakeNativeFramePacketSource {
        stream_info: VideoPacketStreamInfo,
        packets: VecDeque<CompressedVideoPacket>,
        seek_packets: Vec<CompressedVideoPacket>,
        events: Arc<std::sync::Mutex<Vec<&'static str>>>,
    }

    impl FakeNativeFramePacketSource {
        fn with_seek_packets(
            packets: Vec<CompressedVideoPacket>,
            seek_packets: Vec<CompressedVideoPacket>,
            events: Arc<std::sync::Mutex<Vec<&'static str>>>,
        ) -> Self {
            Self {
                stream_info: test_video_packet_stream_info(),
                packets: packets.into(),
                seek_packets,
                events,
            }
        }
    }

    impl MacosNativeFramePacketSource for FakeNativeFramePacketSource {
        fn send_next_packet(
            &mut self,
            decoder_session: &Arc<Mutex<Box<dyn NativeDecoderSession>>>,
        ) -> anyhow::Result<MacosNativeFramePacketSendStatus> {
            let Some(packet) = self.packets.pop_front() else {
                return Ok(MacosNativeFramePacketSendStatus::EndOfStream);
            };
            send_macos_native_frame_packet(decoder_session, packet)?;
            Ok(MacosNativeFramePacketSendStatus::Sent)
        }

        fn seek_to(&mut self, _position: Duration) -> anyhow::Result<()> {
            if let Ok(mut events) = self.events.lock() {
                events.push("packet_seek");
            }
            self.packets = self.seek_packets.clone().into();
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct RecordingNativeDecoderState {
        events: Arc<std::sync::Mutex<Vec<&'static str>>>,
        sent_packets: Vec<DecoderPacket>,
        queued_frames: VecDeque<DecoderReceiveNativeFrameOutput>,
        next_handle: usize,
        released_handles: usize,
        flush_count: usize,
    }

    impl RecordingNativeDecoderState {
        fn shared(events: Arc<std::sync::Mutex<Vec<&'static str>>>) -> Arc<std::sync::Mutex<Self>> {
            Arc::new(std::sync::Mutex::new(Self {
                events,
                next_handle: 100,
                ..Self::default()
            }))
        }
    }

    struct RecordingNativeDecoderSession {
        state: Arc<std::sync::Mutex<RecordingNativeDecoderState>>,
    }

    impl NativeDecoderSession for RecordingNativeDecoderSession {
        fn session_info(&self) -> DecoderSessionInfo {
            DecoderSessionInfo {
                decoder_name: Some("recording-native-decoder".to_owned()),
                selected_hardware_backend: Some("fixture-native".to_owned()),
                output_format: Some(player_plugin::DecoderFrameFormat::Nv12),
            }
        }

        fn send_packet(
            &mut self,
            packet: &DecoderPacket,
            _data: &[u8],
        ) -> Result<DecoderPacketResult, DecoderError> {
            let mut state = self
                .state
                .lock()
                .map_err(|_| DecoderError::internal("recording session state is poisoned"))?;
            if let Ok(mut events) = state.events.lock() {
                events.push(if packet.end_of_stream {
                    "send_eos"
                } else {
                    "send_packet"
                });
            }
            state.sent_packets.push(packet.clone());
            if packet.end_of_stream {
                state
                    .queued_frames
                    .push_back(DecoderReceiveNativeFrameOutput::Eof);
            } else {
                let handle = state.next_handle;
                state.next_handle = state.next_handle.saturating_add(1);
                state
                    .queued_frames
                    .push_back(DecoderReceiveNativeFrameOutput::Frame(test_native_frame(
                        handle,
                        packet.pts_us,
                    )));
            }
            Ok(DecoderPacketResult { accepted: true })
        }

        fn receive_native_frame(
            &mut self,
        ) -> Result<DecoderReceiveNativeFrameOutput, DecoderError> {
            self.state
                .lock()
                .map_err(|_| DecoderError::internal("recording session state is poisoned"))
                .map(|mut state| {
                    state
                        .queued_frames
                        .pop_front()
                        .unwrap_or(DecoderReceiveNativeFrameOutput::NeedMoreInput)
                })
        }

        fn release_native_frame(&mut self, _frame: DecoderNativeFrame) -> Result<(), DecoderError> {
            let mut state = self
                .state
                .lock()
                .map_err(|_| DecoderError::internal("recording session state is poisoned"))?;
            state.released_handles = state.released_handles.saturating_add(1);
            Ok(())
        }

        fn flush(&mut self) -> Result<(), DecoderError> {
            let mut state = self
                .state
                .lock()
                .map_err(|_| DecoderError::internal("recording session state is poisoned"))?;
            if let Ok(mut events) = state.events.lock() {
                events.push("flush");
            }
            state.flush_count = state.flush_count.saturating_add(1);
            state.queued_frames.clear();
            Ok(())
        }

        fn close(&mut self) -> Result<(), DecoderError> {
            Ok(())
        }
    }

    fn native_frame_source_for_test(
        packet_source: FakeNativeFramePacketSource,
        session_state: Arc<std::sync::Mutex<RecordingNativeDecoderState>>,
        outstanding_frames: Arc<AtomicUsize>,
        end_of_input_sent: bool,
        end_of_stream_received: bool,
    ) -> MacosNativeFrameVideoSource {
        let stream_info = packet_source.stream_info.clone();
        let session: Arc<std::sync::Mutex<Box<dyn NativeDecoderSession>>> = Arc::new(
            std::sync::Mutex::new(Box::new(RecordingNativeDecoderSession {
                state: session_state,
            })),
        );
        let shared = Arc::new(std::sync::Mutex::new(MacosNativeFrameDecoderState {
            frame_processor_chain: None,
            presenter: None,
            presentation_epoch: 0,
        }));
        let (command_tx, command_rx) = mpsc::channel();
        let (frame_tx, frame_rx) = mpsc::channel();
        let current_generation = Arc::new(AtomicU64::new(0));
        let buffered_frame_count = Arc::new(AtomicUsize::new(0));
        let prefetch_limit = Arc::new(AtomicUsize::new(1));
        let prefetch_wakeup = Arc::new(MacosNativeFramePrefetchWakeup::default());
        let worker = spawn_macos_native_frame_prefetch_worker(
            Box::new(packet_source),
            session.clone(),
            shared.clone(),
            outstanding_frames.clone(),
            command_rx,
            frame_tx,
            current_generation.clone(),
            buffered_frame_count.clone(),
            prefetch_limit.clone(),
            prefetch_wakeup.clone(),
        )
        .expect("test prefetch worker should spawn");
        MacosNativeFrameVideoSource {
            stream_info,
            session,
            shared,
            outstanding_frames,
            command_tx,
            frame_rx,
            generation: 0,
            current_generation,
            buffered_frame_count,
            prefetch_limit,
            prefetch_wakeup,
            end_of_input_sent,
            end_of_stream_received,
            worker: Some(worker),
        }
    }

    fn contains_ordered_events(events: &[&'static str], expected: &[&'static str]) -> bool {
        let mut next_expected = 0;
        for event in events {
            if expected
                .get(next_expected)
                .is_some_and(|expected| expected == event)
            {
                next_expected += 1;
                if next_expected == expected.len() {
                    return true;
                }
            }
        }
        expected.is_empty()
    }

    fn test_video_packet_stream_info() -> VideoPacketStreamInfo {
        VideoPacketStreamInfo {
            stream_index: 0,
            codec: "H264".to_owned(),
            extradata: Vec::new(),
            width: Some(320),
            height: Some(180),
            frame_rate: Some(24.0),
        }
    }

    fn fake_source_normalizer_packet_stream_info(codec: &str) -> SourceNormalizerPacketStreamInfo {
        SourceNormalizerPacketStreamInfo {
            session_id: Some("fake-session".to_owned()),
            normalizer_name: Some("fake-normalizer".to_owned()),
            runtime_profile: Some("fixture-packet".to_owned()),
            selected_backend: Some("fake".to_owned()),
            tracks: vec![SourceNormalizerPacketTrackInfo {
                stream_index: 0,
                media_kind: SourceNormalizerPacketMediaKind::Video,
                codec: codec.to_owned(),
                extradata: Vec::new(),
                bitstream_format: Some(DecoderBitstreamFormat::Avcc),
                width: Some(320),
                height: Some(180),
                coded_width: Some(320),
                coded_height: Some(180),
                sample_rate: None,
                channels: None,
                frame_rate: Some(24.0),
                time_base_num: Some(1),
                time_base_den: Some(24_000),
            }],
            selected_track_index: Some(0),
            duration_millis: Some(1_000),
            seekable: true,
        }
    }

    fn test_compressed_packet(pts_us: i64) -> CompressedVideoPacket {
        CompressedVideoPacket {
            pts_us: Some(pts_us),
            dts_us: Some(pts_us),
            duration_us: Some(41_667),
            stream_index: 0,
            key_frame: true,
            discontinuity: false,
            data: vec![0, 0, 1, 9],
        }
    }

    fn test_native_frame(handle: usize, pts_us: Option<i64>) -> DecoderNativeFrame {
        DecoderNativeFrame {
            metadata: DecoderNativeFrameMetadata {
                media_kind: DecoderMediaKind::Video,
                format: player_plugin::DecoderFrameFormat::Nv12,
                codec: "H264".to_owned(),
                pts_us,
                duration_us: Some(41_667),
                width: 320,
                height: 180,
                coded_width: Some(320),
                coded_height: Some(180),
                visible_rect: None,
                handle_kind: DecoderNativeHandleKind::CvPixelBuffer,
                frame_id: Some(handle as u64),
                release_tracking: None,
            },
            handle,
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

    #[derive(Debug, Default)]
    struct RecordingFrameProcessorState {
        submit_status: Option<FrameProcessorSubmitStatus>,
        receive_pending: bool,
        output_handle_offset: usize,
        output_requires_release: Option<bool>,
        submit_to_ready_us: Option<u64>,
        forced_queue_depth: Option<u32>,
        forced_in_flight_frames: Option<u32>,
        submitted_handles: Vec<usize>,
        released_handles: Vec<usize>,
        flush_count: usize,
        close_count: usize,
    }

    struct RecordingFrameProcessorSession {
        state: Arc<std::sync::Mutex<RecordingFrameProcessorState>>,
        pending: Option<FrameProcessorOutputFrame>,
    }

    impl RecordingFrameProcessorSession {
        fn new(state: Arc<std::sync::Mutex<RecordingFrameProcessorState>>) -> Self {
            Self {
                state,
                pending: None,
            }
        }
    }

    impl FrameProcessorSession for RecordingFrameProcessorSession {
        fn session_info(&self) -> FrameProcessorSessionInfo {
            FrameProcessorSessionInfo {
                processor_name: Some("recording-frame-processor".to_owned()),
                selected_backend: Some("fixture".to_owned()),
                output_handle_kind: Some(player_plugin::NativeHandleKind::CvPixelBuffer),
                max_in_flight_frames: Some(1),
            }
        }

        fn submit_frame(
            &mut self,
            frame: &NativeFrame,
            _submit: &FrameProcessorSubmitFrame,
        ) -> Result<FrameProcessorSubmitResult, FrameProcessorError> {
            let mut state = self
                .state
                .lock()
                .map_err(|_| FrameProcessorError::internal("recording processor poisoned"))?;
            state.submitted_handles.push(frame.handle);
            if let Some(status) = state.submit_status {
                return Ok(FrameProcessorSubmitResult {
                    status,
                    queue_depth: Some(state.forced_queue_depth.unwrap_or(0)),
                    in_flight_frames: Some(state.forced_in_flight_frames.unwrap_or(0)),
                    message: Some("forced submit status".to_owned()),
                });
            }
            if state.receive_pending {
                return Ok(FrameProcessorSubmitResult::default());
            }
            let mut output_metadata = frame.metadata.clone();
            output_metadata.frame_id = output_metadata
                .frame_id
                .map(|frame_id| frame_id.saturating_add(10_000));
            if let Some(requires_release) = state.output_requires_release {
                output_metadata.release_tracking =
                    Some(player_plugin::NativeFrameReleaseTracking {
                        frame_id: output_metadata.frame_id,
                        requires_release,
                    });
            }
            let output_handle = state.output_handle_offset.saturating_add(frame.handle);
            self.pending = Some(FrameProcessorOutputFrame {
                frame: NativeFrame {
                    metadata: output_metadata,
                    handle: output_handle,
                },
                timings: FrameProcessorFrameTimings {
                    queue_wait_us: Some(0),
                    process_time_us: state.submit_to_ready_us,
                    submit_to_ready_us: state.submit_to_ready_us.or(Some(100)),
                },
                source_frame_id: frame.metadata.frame_id,
            });
            Ok(FrameProcessorSubmitResult::default())
        }

        fn receive_frame(&mut self) -> Result<FrameProcessorReceiveOutput, FrameProcessorError> {
            let receive_pending = self
                .state
                .lock()
                .map_err(|_| FrameProcessorError::internal("recording processor poisoned"))?
                .receive_pending;
            if receive_pending {
                Ok(FrameProcessorReceiveOutput::Pending)
            } else if let Some(output) = self.pending.take() {
                Ok(FrameProcessorReceiveOutput::Frame(output))
            } else {
                Ok(FrameProcessorReceiveOutput::Pending)
            }
        }

        fn release_frame(&mut self, frame: NativeFrame) -> Result<(), FrameProcessorError> {
            let mut state = self
                .state
                .lock()
                .map_err(|_| FrameProcessorError::internal("recording processor poisoned"))?;
            state.released_handles.push(frame.handle);
            Ok(())
        }

        fn flush(&mut self) -> Result<(), FrameProcessorError> {
            self.pending = None;
            let mut state = self
                .state
                .lock()
                .map_err(|_| FrameProcessorError::internal("recording processor poisoned"))?;
            state.flush_count = state.flush_count.saturating_add(1);
            Ok(())
        }

        fn close(&mut self) -> Result<(), FrameProcessorError> {
            let mut state = self
                .state
                .lock()
                .map_err(|_| FrameProcessorError::internal("recording processor poisoned"))?;
            state.close_count = state.close_count.saturating_add(1);
            Ok(())
        }
    }

    fn frame_processor_chain_for_test(
        mode: FrameProcessorMode,
        sessions: Vec<RecordingFrameProcessorSession>,
    ) -> MacosFrameProcessorChain {
        MacosFrameProcessorChain {
            processors: sessions
                .into_iter()
                .enumerate()
                .map(|(processor_index, session)| MacosFrameProcessorNode {
                    plugin_name: format!("recording-frame-processor-{processor_index}"),
                    processor_index,
                    session: Box::new(session),
                })
                .collect(),
            mode,
            policy: FrameProcessorPolicy {
                frame_deadline: Duration::from_millis(16),
                late_output_tolerance: Duration::from_millis(4),
                max_chain_depth: 8,
                max_in_flight_frames_per_processor: 1,
            },
            metrics: PlayerFrameProcessingMetrics::default(),
            pending_events: VecDeque::new(),
            debug: FrameProcessorDebugState::from_env(),
        }
    }
}
