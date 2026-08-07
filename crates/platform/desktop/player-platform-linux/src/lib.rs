//! Linux desktop runtime adapter.
//!
//! This crate keeps the Linux host API available across targets for workspace
//! integration tests. Actual runtime initialization is supported only on Linux;
//! other targets return `Unsupported` before touching platform resources.

#![allow(
    clippy::result_large_err,
    reason = "PlayerError is a shared public API; boxing Linux platform errors would change public signatures"
)]

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use player_model::MediaSource;
use player_platform_desktop::{
    diagnostics::{append_plugin_diagnostics, unsupported_plugin_diagnostic},
    open_platform_desktop_source_with_options_and_interrupt,
    probe_platform_desktop_source_with_options,
};
use player_runtime::{
    FrameProcessorMode, PlayerDecoderPluginVideoMode, PlayerError, PlayerErrorCode,
    PlayerMediaInfo, PlayerPluginDiagnostic, PlayerPluginDiagnosticStatus, PlayerResult,
    PlayerRuntime, PlayerRuntimeAdapterBootstrap, PlayerRuntimeAdapterCapabilities,
    PlayerRuntimeAdapterFactory, PlayerRuntimeAdapterInitializer, PlayerRuntimeBootstrap,
    PlayerRuntimeInitializer, PlayerRuntimeOptions, PlayerRuntimeStartup, SourceNormalizerMode,
    register_default_runtime_adapter_factory,
};

pub const LINUX_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID: &str = "linux_software_desktop";

#[derive(Debug, Clone)]
pub struct LinuxHostRuntimeProbe {
    pub adapter_id: &'static str,
    pub capabilities: PlayerRuntimeAdapterCapabilities,
    pub media_info: PlayerMediaInfo,
    pub startup: PlayerRuntimeStartup,
}

pub fn linux_runtime_adapter_factory() -> &'static dyn PlayerRuntimeAdapterFactory {
    static FACTORY: LinuxSoftwarePlayerRuntimeAdapterFactory =
        LinuxSoftwarePlayerRuntimeAdapterFactory;
    &FACTORY
}

pub fn install_default_linux_runtime_adapter_factory() -> PlayerResult<()> {
    register_default_runtime_adapter_factory(linux_runtime_adapter_factory())
}

pub fn open_linux_host_runtime_uri_with_options(
    uri: impl Into<String>,
    options: PlayerRuntimeOptions,
) -> PlayerResult<PlayerRuntimeBootstrap> {
    open_linux_host_runtime_source_with_options(MediaSource::new(uri), options)
}

pub fn open_linux_host_runtime_uri_with_options_and_interrupt(
    uri: impl Into<String>,
    options: PlayerRuntimeOptions,
    interrupt_flag: Arc<AtomicBool>,
) -> PlayerResult<PlayerRuntimeBootstrap> {
    open_linux_host_runtime_source_with_options_and_interrupt(
        MediaSource::new(uri),
        options,
        interrupt_flag,
    )
}

pub fn probe_linux_host_runtime_uri_with_options(
    uri: impl Into<String>,
    options: PlayerRuntimeOptions,
) -> PlayerResult<LinuxHostRuntimeProbe> {
    probe_linux_host_runtime_source_with_options(MediaSource::new(uri), options)
}

pub fn probe_linux_host_runtime_source_with_options(
    source: MediaSource,
    options: PlayerRuntimeOptions,
) -> PlayerResult<LinuxHostRuntimeProbe> {
    if !cfg!(target_os = "linux") {
        return Err(PlayerError::new(
            PlayerErrorCode::Unsupported,
            "linux host runtime strategy can only be probed on Linux targets",
        ));
    }

    let initializer = PlayerRuntimeInitializer::probe_source_with_factory(
        source,
        options.clone(),
        linux_runtime_adapter_factory(),
    )?;
    let startup =
        linux_startup_with_unsupported_plugin_diagnostics(initializer.startup(), &options);

    Ok(LinuxHostRuntimeProbe {
        adapter_id: LINUX_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
        capabilities: initializer.capabilities(),
        media_info: initializer.media_info(),
        startup,
    })
}

pub fn open_linux_host_runtime_source_with_options(
    source: MediaSource,
    options: PlayerRuntimeOptions,
) -> PlayerResult<PlayerRuntimeBootstrap> {
    if !cfg!(target_os = "linux") {
        return Err(PlayerError::new(
            PlayerErrorCode::Unsupported,
            "linux host runtime strategy can only be initialized on Linux targets",
        ));
    }

    PlayerRuntime::open_source_with_factory(source, options, linux_runtime_adapter_factory())
}

pub fn open_linux_host_runtime_source_with_options_and_interrupt(
    source: MediaSource,
    options: PlayerRuntimeOptions,
    interrupt_flag: Arc<AtomicBool>,
) -> PlayerResult<PlayerRuntimeBootstrap> {
    if !cfg!(target_os = "linux") {
        return Err(PlayerError::new(
            PlayerErrorCode::Unsupported,
            "linux host runtime strategy can only be initialized on Linux targets",
        ));
    }

    let mut bootstrap = open_platform_desktop_source_with_options_and_interrupt(
        LINUX_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
        source,
        options.clone(),
        interrupt_flag,
    )?;
    bootstrap.startup =
        linux_startup_with_unsupported_plugin_diagnostics(bootstrap.startup, &options);
    Ok(PlayerRuntime::from_adapter_bootstrap_with_pipeline(
        LINUX_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
        bootstrap,
        options.pipeline_event_dispatcher.clone(),
        options.pipeline_event_platform.clone(),
    ))
}

#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxSoftwarePlayerRuntimeAdapterFactory;

impl PlayerRuntimeAdapterFactory for LinuxSoftwarePlayerRuntimeAdapterFactory {
    fn adapter_id(&self) -> &'static str {
        LINUX_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
    }

    fn probe_source_with_options(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
    ) -> PlayerResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
        if !cfg!(target_os = "linux") {
            return Err(PlayerError::new(
                PlayerErrorCode::Unsupported,
                "linux desktop adapter can only be initialized on Linux desktop targets",
            ));
        }

        let initializer = probe_platform_desktop_source_with_options(
            LINUX_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
            source,
            options.clone(),
        )?;
        Ok(Box::new(LinuxRuntimeAdapterInitializer {
            inner: initializer,
            options,
        }))
    }
}

struct LinuxRuntimeAdapterInitializer {
    inner: Box<dyn PlayerRuntimeAdapterInitializer>,
    options: PlayerRuntimeOptions,
}

impl PlayerRuntimeAdapterInitializer for LinuxRuntimeAdapterInitializer {
    fn capabilities(&self) -> PlayerRuntimeAdapterCapabilities {
        self.inner.capabilities()
    }

    fn media_info(&self) -> PlayerMediaInfo {
        self.inner.media_info()
    }

    fn startup(&self) -> PlayerRuntimeStartup {
        linux_startup_with_unsupported_plugin_diagnostics(self.inner.startup(), &self.options)
    }

    fn initialize(self: Box<Self>) -> PlayerResult<PlayerRuntimeAdapterBootstrap> {
        self.inner.initialize()
    }
}

fn linux_startup_with_unsupported_plugin_diagnostics(
    startup: PlayerRuntimeStartup,
    options: &PlayerRuntimeOptions,
) -> PlayerRuntimeStartup {
    append_plugin_diagnostics(startup, &linux_unsupported_plugin_diagnostics(options))
}

fn linux_unsupported_plugin_diagnostics(
    options: &PlayerRuntimeOptions,
) -> Vec<PlayerPluginDiagnostic> {
    let mut diagnostics = Vec::new();
    if !options.decoder_plugin_library_paths.is_empty()
        || options.decoder_plugin_video_mode == PlayerDecoderPluginVideoMode::PreferNativeFrame
    {
        diagnostics.push(linux_unsupported_plugin_diagnostic(
            "decoder",
            PlayerPluginDiagnosticStatus::DecoderUnsupported,
            "Linux desktop runtime does not currently inspect decoder plugins or provide native-frame playback diagnostics",
        ));
    }
    if options.source_normalizer_mode != SourceNormalizerMode::Disabled
        || !options.source_normalizer_plugin_library_paths.is_empty()
    {
        diagnostics.push(linux_unsupported_plugin_diagnostic(
            "source_normalizer",
            PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
            "Linux desktop runtime does not currently inspect source-normalizer plugins",
        ));
    }
    if options.frame_processor_mode != FrameProcessorMode::Disabled
        || !options.frame_processor_library_paths.is_empty()
    {
        diagnostics.push(linux_unsupported_plugin_diagnostic(
            "frame_processor",
            PlayerPluginDiagnosticStatus::FrameProcessorUnsupported,
            "Linux desktop runtime does not currently inspect frame-processor plugins or provide a native-frame processor chain",
        ));
    }
    diagnostics
}

fn linux_unsupported_plugin_diagnostic(
    plugin_kind: &str,
    status: PlayerPluginDiagnosticStatus,
    message: &str,
) -> PlayerPluginDiagnostic {
    unsupported_plugin_diagnostic(
        plugin_kind,
        status,
        message,
        "linuxDesktopPluginDiagnostics",
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        LINUX_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID, LinuxSoftwarePlayerRuntimeAdapterFactory,
        linux_unsupported_plugin_diagnostics, open_linux_host_runtime_source_with_options,
        probe_linux_host_runtime_source_with_options,
    };
    use player_model::MediaSource;
    use player_runtime::{
        FrameProcessorMode, PlayerDecoderPluginVideoMode, PlayerErrorCode,
        PlayerPluginDiagnosticStatus, PlayerRuntimeAdapterBackendFamily,
        PlayerRuntimeAdapterFactory, PlayerRuntimeOptions, SourceNormalizerMode,
    };

    #[test]
    fn linux_factory_matches_host_support() {
        let factory = LinuxSoftwarePlayerRuntimeAdapterFactory;

        if cfg!(target_os = "linux") {
            let Some(test_video_path) = test_video_path() else {
                eprintln!(
                    "skipping Linux fixture-backed test: fixtures/media/tiny-h264-aac.m4v is unavailable"
                );
                return;
            };
            let result = factory.probe_source_with_options(
                MediaSource::new(test_video_path),
                PlayerRuntimeOptions::default(),
            );
            let initializer = result.expect("linux host should support the linux desktop adapter");
            let capabilities = initializer.capabilities();
            assert_eq!(
                capabilities.adapter_id,
                LINUX_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
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
                Ok(_) => panic!("non-linux hosts should reject the linux adapter"),
                Err(error) => error,
            };
            assert_eq!(error.code(), PlayerErrorCode::Unsupported);
        }
    }

    #[test]
    fn linux_host_probe_matches_factory_support() {
        if cfg!(target_os = "linux") {
            let Some(test_video_path) = test_video_path() else {
                eprintln!(
                    "skipping Linux fixture-backed test: fixtures/media/tiny-h264-aac.m4v is unavailable"
                );
                return;
            };
            let result = probe_linux_host_runtime_source_with_options(
                MediaSource::new(test_video_path),
                PlayerRuntimeOptions::default(),
            );
            let probe = result.expect("linux host should support the linux host runtime probe");
            assert_eq!(probe.adapter_id, LINUX_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID);
            assert_eq!(
                probe.capabilities.backend_family,
                PlayerRuntimeAdapterBackendFamily::SoftwareDesktop
            );
        } else {
            let result = probe_linux_host_runtime_source_with_options(
                MediaSource::new("fixture.mp4"),
                PlayerRuntimeOptions::default(),
            );
            let error = result.expect_err("non-linux hosts should reject the linux host probe");
            assert_eq!(error.code(), PlayerErrorCode::Unsupported);
        }
    }

    #[test]
    fn linux_host_open_matches_factory_support() {
        if cfg!(target_os = "linux") {
            let Some(test_video_path) = test_video_path() else {
                eprintln!(
                    "skipping Linux fixture-backed test: fixtures/media/tiny-h264-aac.m4v is unavailable"
                );
                return;
            };
            let result = open_linux_host_runtime_source_with_options(
                MediaSource::new(test_video_path),
                PlayerRuntimeOptions::default(),
            );
            let bootstrap =
                result.expect("linux host should support the linux host runtime open helper");
            assert_eq!(
                bootstrap.runtime.adapter_id(),
                LINUX_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
            );
        } else {
            let result = open_linux_host_runtime_source_with_options(
                MediaSource::new("fixture.mp4"),
                PlayerRuntimeOptions::default(),
            );
            let error = match result {
                Ok(_) => panic!("non-linux hosts should reject the linux host opener"),
                Err(error) => error,
            };
            assert_eq!(error.code(), PlayerErrorCode::Unsupported);
        }
    }

    #[test]
    fn linux_configured_plugin_paths_emit_unsupported_diagnostics() {
        let diagnostics = linux_unsupported_plugin_diagnostics(
            &PlayerRuntimeOptions::default()
                .with_decoder_plugin_video_mode(PlayerDecoderPluginVideoMode::PreferNativeFrame)
                .with_decoder_plugin_library_paths([std::path::PathBuf::from("/tmp/fake-decoder")])
                .with_source_normalizer_mode(SourceNormalizerMode::PreferNormalized)
                .with_source_normalizer_plugin_library_paths([std::path::PathBuf::from(
                    "/tmp/fake-normalizer",
                )])
                .with_frame_processor_mode(FrameProcessorMode::RequireProcessed)
                .with_frame_processor_library_paths([std::path::PathBuf::from(
                    "/tmp/fake-frame-processor",
                )]),
        );

        assert_eq!(diagnostics.len(), 3);
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.status == PlayerPluginDiagnosticStatus::DecoderUnsupported
                && diagnostic.plugin_kind.as_deref() == Some("decoder")
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.status == PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported
                && diagnostic.plugin_kind.as_deref() == Some("source_normalizer")
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.status == PlayerPluginDiagnosticStatus::FrameProcessorUnsupported
                && diagnostic.plugin_kind.as_deref() == Some("frame_processor")
        }));
        assert!(diagnostics.iter().all(|diagnostic| {
            diagnostic.details.iter().any(|detail| {
                detail.key == "linuxDesktopPluginDiagnostics" && detail.value == "unsupported"
            })
        }));
    }

    fn test_video_path() -> Option<String> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../fixtures/media/tiny-h264-aac.m4v");
        path.canonicalize()
            .ok()
            .map(|path| path.to_string_lossy().into_owned())
    }
}
