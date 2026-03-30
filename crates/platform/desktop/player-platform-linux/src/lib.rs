use player_core::MediaSource;
use player_platform_desktop::probe_platform_desktop_source_with_options;
use player_runtime::{
    PlayerMediaInfo, PlayerRuntime, PlayerRuntimeAdapterCapabilities, PlayerRuntimeAdapterFactory,
    PlayerRuntimeAdapterInitializer, PlayerRuntimeBootstrap, PlayerRuntimeError,
    PlayerRuntimeErrorCode, PlayerRuntimeInitializer, PlayerRuntimeOptions, PlayerRuntimeResult,
    PlayerRuntimeStartup, register_default_runtime_adapter_factory,
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

pub fn install_default_linux_runtime_adapter_factory() -> PlayerRuntimeResult<()> {
    register_default_runtime_adapter_factory(linux_runtime_adapter_factory())
}

pub fn open_linux_host_runtime_uri_with_options(
    uri: impl Into<String>,
    options: PlayerRuntimeOptions,
) -> PlayerRuntimeResult<PlayerRuntimeBootstrap> {
    open_linux_host_runtime_source_with_options(MediaSource::new(uri), options)
}

pub fn probe_linux_host_runtime_uri_with_options(
    uri: impl Into<String>,
    options: PlayerRuntimeOptions,
) -> PlayerRuntimeResult<LinuxHostRuntimeProbe> {
    probe_linux_host_runtime_source_with_options(MediaSource::new(uri), options)
}

pub fn probe_linux_host_runtime_source_with_options(
    source: MediaSource,
    options: PlayerRuntimeOptions,
) -> PlayerRuntimeResult<LinuxHostRuntimeProbe> {
    if !cfg!(target_os = "linux") {
        return Err(PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::Unsupported,
            "linux host runtime strategy can only be probed on Linux targets",
        ));
    }

    let initializer = PlayerRuntimeInitializer::probe_source_with_factory(
        source,
        options,
        linux_runtime_adapter_factory(),
    )?;

    Ok(LinuxHostRuntimeProbe {
        adapter_id: LINUX_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
        capabilities: initializer.capabilities(),
        media_info: initializer.media_info(),
        startup: initializer.startup(),
    })
}

pub fn open_linux_host_runtime_source_with_options(
    source: MediaSource,
    options: PlayerRuntimeOptions,
) -> PlayerRuntimeResult<PlayerRuntimeBootstrap> {
    if !cfg!(target_os = "linux") {
        return Err(PlayerRuntimeError::new(
            PlayerRuntimeErrorCode::Unsupported,
            "linux host runtime strategy can only be initialized on Linux targets",
        ));
    }

    PlayerRuntime::open_source_with_factory(source, options, linux_runtime_adapter_factory())
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
    ) -> PlayerRuntimeResult<Box<dyn PlayerRuntimeAdapterInitializer>> {
        if !cfg!(target_os = "linux") {
            return Err(PlayerRuntimeError::new(
                PlayerRuntimeErrorCode::Unsupported,
                "linux desktop adapter can only be initialized on Linux desktop targets",
            ));
        }

        probe_platform_desktop_source_with_options(
            LINUX_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID,
            source,
            options,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LINUX_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID, LinuxSoftwarePlayerRuntimeAdapterFactory,
        open_linux_host_runtime_source_with_options, probe_linux_host_runtime_source_with_options,
    };
    use player_core::MediaSource;
    use player_runtime::{
        PlayerRuntimeAdapterBackendFamily, PlayerRuntimeAdapterFactory, PlayerRuntimeErrorCode,
        PlayerRuntimeOptions,
    };

    #[test]
    fn linux_factory_matches_host_support() {
        let factory = LinuxSoftwarePlayerRuntimeAdapterFactory;
        let result = factory.probe_source_with_options(
            MediaSource::new(test_video_path()),
            PlayerRuntimeOptions::default(),
        );

        if cfg!(target_os = "linux") {
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
            let error = match result {
                Ok(_) => panic!("non-linux hosts should reject the linux adapter"),
                Err(error) => error,
            };
            assert_eq!(error.code(), PlayerRuntimeErrorCode::Unsupported);
        }
    }

    #[test]
    fn linux_host_probe_matches_factory_support() {
        let result = probe_linux_host_runtime_source_with_options(
            MediaSource::new(test_video_path()),
            PlayerRuntimeOptions::default(),
        );

        if cfg!(target_os = "linux") {
            let probe = result.expect("linux host should support the linux host runtime probe");
            assert_eq!(probe.adapter_id, LINUX_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID);
            assert_eq!(
                probe.capabilities.backend_family,
                PlayerRuntimeAdapterBackendFamily::SoftwareDesktop
            );
        } else {
            let error = result.expect_err("non-linux hosts should reject the linux host probe");
            assert_eq!(error.code(), PlayerRuntimeErrorCode::Unsupported);
        }
    }

    #[test]
    fn linux_host_open_matches_factory_support() {
        let result = open_linux_host_runtime_source_with_options(
            MediaSource::new(test_video_path()),
            PlayerRuntimeOptions::default(),
        );

        if cfg!(target_os = "linux") {
            let bootstrap =
                result.expect("linux host should support the linux host runtime open helper");
            assert_eq!(
                bootstrap.runtime.adapter_id(),
                LINUX_SOFTWARE_PLAYER_RUNTIME_ADAPTER_ID
            );
        } else {
            let error = match result {
                Ok(_) => panic!("non-linux hosts should reject the linux host opener"),
                Err(error) => error,
            };
            assert_eq!(error.code(), PlayerRuntimeErrorCode::Unsupported);
        }
    }

    fn test_video_path() -> String {
        format!("{}/../../../../test-video.mp4", env!("CARGO_MANIFEST_DIR"))
    }
}
