use player_core::MediaSource;
use player_platform_desktop::probe_platform_desktop_source_with_options;
use player_runtime::{
    PlayerMediaInfo, PlayerRuntime, PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory,
    PlayerRuntimeAdapterInitializer, PlayerRuntimeBootstrap, PlayerRuntimeError,
    PlayerRuntimeErrorCode, PlayerRuntimeInitializer, PlayerRuntimeOptions, PlayerRuntimeResult,
    PlayerRuntimeStartup, register_default_runtime_adapter_factory,
};

pub const WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID: &str = "windows_software_desktop";

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

        probe_platform_desktop_source_with_options(
            WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
            source,
            options,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID, WindowsSoftwarePlayerRuntimeAdapterFactory,
        open_windows_host_runtime_source_with_options,
        probe_windows_host_runtime_source_with_options,
    };
    use player_core::MediaSource;
    use player_runtime::{
        PlayerRuntimeAdapterBackendFamily, PlayerRuntimeAdapterFactory, PlayerRuntimeErrorCode,
        PlayerRuntimeOptions,
    };

    #[test]
    fn windows_factory_matches_host_support() {
        let factory = WindowsSoftwarePlayerRuntimeAdapterFactory;
        let result = factory.probe_source_with_options(
            MediaSource::new(test_video_path()),
            PlayerRuntimeOptions::default(),
        );

        if cfg!(target_os = "windows") {
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
            let error = match result {
                Ok(_) => panic!("non-windows hosts should reject the windows adapter"),
                Err(error) => error,
            };
            assert_eq!(error.code(), PlayerRuntimeErrorCode::Unsupported);
        }
    }

    #[test]
    fn windows_host_probe_matches_factory_support() {
        let result = probe_windows_host_runtime_source_with_options(
            MediaSource::new(test_video_path()),
            PlayerRuntimeOptions::default(),
        );

        if cfg!(target_os = "windows") {
            let probe = result.expect("windows host should support the windows host runtime probe");
            assert_eq!(probe.adapter_id, WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID);
            assert_eq!(
                probe.capabilities.backend_family,
                PlayerRuntimeAdapterBackendFamily::SoftwareDesktop
            );
        } else {
            let error = result.expect_err("non-windows hosts should reject the windows host probe");
            assert_eq!(error.code(), PlayerRuntimeErrorCode::Unsupported);
        }
    }

    #[test]
    fn windows_host_open_matches_factory_support() {
        let result = open_windows_host_runtime_source_with_options(
            MediaSource::new(test_video_path()),
            PlayerRuntimeOptions::default(),
        );

        if cfg!(target_os = "windows") {
            let bootstrap =
                result.expect("windows host should support the windows host runtime open helper");
            assert_eq!(
                bootstrap.runtime.adapter_id(),
                WINDOWS_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
            );
        } else {
            let error = match result {
                Ok(_) => panic!("non-windows hosts should reject the windows host opener"),
                Err(error) => error,
            };
            assert_eq!(error.code(), PlayerRuntimeErrorCode::Unsupported);
        }
    }

    fn test_video_path() -> String {
        format!("{}/../../../../test-video.mp4", env!("CARGO_MANIFEST_DIR"))
    }
}
