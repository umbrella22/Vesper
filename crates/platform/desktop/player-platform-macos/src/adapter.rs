use super::*;

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

pub(crate) fn open_macos_software_runtime_with_prepared_normalization(
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
                .push(source_normalizer_runtime_diagnostic(
                    None,
                    message,
                    PlayerPluginParticipation::Bypassed,
                ));
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
                .push(source_normalizer_runtime_diagnostic(
                    None,
                    message,
                    PlayerPluginParticipation::Bypassed,
                ));
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
pub(crate) enum MacosHostRuntimeSelection {
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

pub(crate) struct MacosHostRuntimeAdapterInitializer {
    pub(crate) selection: MacosHostRuntimeSelection,
    pub(crate) capabilities: PlayerRuntimeAdapterCapabilities,
    pub(crate) media_info: PlayerMediaInfo,
    pub(crate) startup: PlayerRuntimeStartup,
}

pub(crate) trait MacosHostFallbackFactory: Send + Sync {
    fn probe_source_with_options(
        &self,
        source: MediaSource,
        options: PlayerRuntimeOptions,
    ) -> PlayerResult<Box<dyn PlayerRuntimeAdapterInitializer>>;
}

#[derive(Debug, Default)]
pub(crate) struct MacosSoftwareFallbackFactory;

#[derive(Debug, Clone)]
pub(crate) struct MacosRuntimeDiagnostics {
    pub(crate) video_decode: PlayerVideoDecodeInfo,
    pub(crate) plugin_diagnostics: Vec<PlayerPluginDiagnostic>,
    pub(crate) has_video_surface: bool,
}

pub(crate) struct MacosRuntimeAdapterInitializer {
    pub(crate) inner: Box<dyn PlayerRuntimeAdapterInitializer>,
    pub(crate) diagnostics: MacosRuntimeDiagnostics,
    pub(crate) fallback: Option<MacosRuntimeAdapterFallback>,
    pub(crate) runtime_fallback: Option<MacosRuntimeActiveFallback>,
    pub(crate) strict_frame_processor_error_prefix: Option<String>,
}

pub(crate) struct MacosRuntimeAdapterFallback {
    pub(crate) inner: Box<dyn PlayerRuntimeAdapterInitializer>,
    pub(crate) diagnostics: MacosRuntimeDiagnostics,
    pub(crate) fallback_reason: String,
}

pub(crate) struct MacosSourceNormalizationOutcome {
    pub(crate) source: MediaSource,
    pub(crate) packet_session: Option<Arc<Mutex<Option<Box<dyn SourceNormalizerPacketSession>>>>>,
    pub(crate) packet_stream_info: Option<player_plugin::SourceNormalizerPacketStreamInfo>,
    pub(crate) diagnostics: Vec<PlayerPluginDiagnostic>,
    pub(crate) selected_profile: Option<String>,
    pub(crate) normalized_endpoint: Option<String>,
    pub(crate) ready_latency: Option<Duration>,
}

impl MacosSourceNormalizationOutcome {
    pub(crate) fn has_packet_stream(&self) -> bool {
        self.packet_session.is_some() && self.packet_stream_info.is_some()
    }
}

#[derive(Clone)]
pub(crate) struct MacosRuntimeActiveFallback {
    pub(crate) source: MediaSource,
    pub(crate) options: PlayerRuntimeOptions,
    pub(crate) fallback_reason: String,
}

pub(crate) struct MacosRuntimeAdapter {
    pub(crate) inner: Box<dyn PlayerRuntimeAdapter>,
    pub(crate) video_decode: PlayerVideoDecodeInfo,
    pub(crate) plugin_diagnostics: Vec<PlayerPluginDiagnostic>,
    pub(crate) has_video_surface: bool,
    pub(crate) runtime_fallback: Option<MacosRuntimeActiveFallback>,
    pub(crate) pending_runtime_fallback_events: VecDeque<PlayerRuntimeEvent>,
    #[allow(dead_code)]
    pub(crate) source_normalizer_packet_session:
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

pub(crate) fn wrap_macos_runtime_bootstrap(
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
    pub(crate) fn activate_runtime_fallback(
        &mut self,
        runtime_error_message: &str,
    ) -> PlayerResult<()> {
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

    pub(crate) fn activate_runtime_fallback_with(
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
            runtime.dispatch(PlayerRuntimeCommand::SeekTo {
                position: progress.position(),
            })?;
        }
        if (playback_rate - 1.0).abs() > f32::EPSILON {
            runtime.dispatch(PlayerRuntimeCommand::SetPlaybackRate {
                rate: playback_rate,
            })?;
        }
        if was_playing {
            runtime.dispatch(PlayerRuntimeCommand::Play)?;
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
