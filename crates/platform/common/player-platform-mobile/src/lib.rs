#![deny(unsafe_code)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use player_model::MediaSource;
use player_plugin::{
    DecoderBitstreamFormat, SourceNormalizerNormalizeLevel, SourceNormalizerOutputRoute,
    SourceNormalizerPacketMediaKind, SourceNormalizerPacketSessionConfig,
    SourceNormalizerResourceCachePolicy, SourceNormalizerResourceSession,
    SourceNormalizerResourceSessionConfig, SourceNormalizerResourceSessionInfo,
    SourceNormalizerResourceSessionState, SourceNormalizerResourceSessionStatus,
};
use player_plugin_loader::{
    FrameProcessorPluginCapabilitySummary, LoadedDynamicPlugin, PluginCapabilitySummary,
    PluginDiagnosticRecord, PluginDiagnosticStatus, PluginRegistry,
    SourceNormalizerPacketPluginCapabilitySummary, SourceNormalizerResourcePluginCapabilitySummary,
};
use player_runtime::{
    FrameProcessorMode, PlayerPluginCapabilitySummary, PlayerPluginDiagnostic,
    PlayerPluginDiagnosticStatus, PlayerPluginFrameProcessorCapabilitySummary,
    PlayerPluginParticipation, PlayerPluginSourceNormalizerCapabilitySummary, PlayerRuntimeOptions,
    PlayerRuntimeStartup, SourceNormalizerMode,
};
use serde::Serialize;

const SOURCE_NORMALIZER_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const SOURCE_NORMALIZER_SESSION_TIMEOUT: Duration = Duration::from_secs(30);
const SOURCE_NORMALIZER_RESOURCE_READY_TIMEOUT: Duration = Duration::from_secs(10);
const SOURCE_NORMALIZER_RESOURCE_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobileSourceNormalizerConfiguration {
    pub mode: SourceNormalizerMode,
    pub plugin_library_paths: Vec<PathBuf>,
    pub runtime_profile: Option<String>,
}

impl Default for MobileSourceNormalizerConfiguration {
    fn default() -> Self {
        Self {
            mode: SourceNormalizerMode::Disabled,
            plugin_library_paths: Vec::new(),
            runtime_profile: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobileFrameProcessorConfiguration {
    pub mode: FrameProcessorMode,
    pub plugin_library_paths: Vec<PathBuf>,
}

impl Default for MobileFrameProcessorConfiguration {
    fn default() -> Self {
        Self {
            mode: FrameProcessorMode::Disabled,
            plugin_library_paths: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MobilePluginConfiguration {
    pub source_normalizer: MobileSourceNormalizerConfiguration,
    pub frame_processor: MobileFrameProcessorConfiguration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobileSourceNormalizerRouteDecision {
    NativeFirst,
    Force,
}

pub struct MobileSourceNormalizerResourceOpen {
    pub plugin_name: Option<String>,
    pub plugin_path: String,
    pub session: Box<dyn SourceNormalizerResourceSession>,
    pub info: SourceNormalizerResourceSessionInfo,
    pub status: SourceNormalizerResourceSessionStatus,
    pub cache_policy: SourceNormalizerResourceCachePolicy,
    pub diagnostics: Vec<PlayerPluginDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileSourceNormalizerResourceWire {
    pub handle: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
    pub plugin_path: String,
    pub output_route: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_profile: Option<String>,
    pub container: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_resource_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playback_uri: Option<String>,
    pub resources: Vec<MobileSourceNormalizerResourceInfoWire>,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk_bytes_used: Option<u64>,
    pub cache_policy: MobileSourceNormalizerCachePolicyWire,
    pub diagnostics: Vec<MobilePluginDiagnosticOwnedWire>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileSourceNormalizerResourceInfoWire {
    pub role: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_length: Option<u64>,
    pub growing: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileSourceNormalizerCachePolicyWire {
    pub session_read_buffer_bytes: u64,
    pub manifest_snapshot_bytes: u64,
    pub session_disk_soft_cap_bytes: u64,
    pub global_disk_soft_cap_bytes: u64,
}

impl MobilePluginConfiguration {
    pub fn from_runtime_options(options: &PlayerRuntimeOptions) -> Self {
        Self {
            source_normalizer: MobileSourceNormalizerConfiguration {
                mode: options.source_normalizer_mode,
                plugin_library_paths: options.source_normalizer_plugin_library_paths.clone(),
                runtime_profile: None,
            },
            frame_processor: MobileFrameProcessorConfiguration {
                mode: options.frame_processor_mode,
                plugin_library_paths: options.frame_processor_library_paths.clone(),
            },
        }
    }

    pub fn apply_to_runtime_options(&self, options: &mut PlayerRuntimeOptions) {
        options.source_normalizer_mode = self.source_normalizer.mode;
        options.source_normalizer_plugin_library_paths =
            self.source_normalizer.plugin_library_paths.clone();
        options.frame_processor_mode = self.frame_processor.mode;
        options.frame_processor_library_paths = self.frame_processor.plugin_library_paths.clone();
    }
}

pub fn apply_mobile_plugin_diagnostics(
    mut startup: PlayerRuntimeStartup,
    source: &MediaSource,
    configuration: &MobilePluginConfiguration,
) -> PlayerRuntimeStartup {
    startup
        .plugin_diagnostics
        .extend(source_normalizer_diagnostics(
            source,
            &configuration.source_normalizer,
        ));
    startup
        .plugin_diagnostics
        .extend(frame_processor_diagnostics(&configuration.frame_processor));
    startup
}

pub fn open_mobile_source_normalizer_resource(
    source: &MediaSource,
    configuration: &MobileSourceNormalizerConfiguration,
    output_root: impl Into<String>,
    decision: MobileSourceNormalizerRouteDecision,
) -> Result<Option<MobileSourceNormalizerResourceOpen>, String> {
    if !matches!(
        configuration.mode,
        SourceNormalizerMode::PreferNormalized | SourceNormalizerMode::RequireNormalized
    ) {
        return Ok(None);
    }
    if decision == MobileSourceNormalizerRouteDecision::NativeFirst
        && native_first_source_normalizer_bypass(source)
        && configuration.mode != SourceNormalizerMode::RequireNormalized
    {
        return Ok(None);
    }

    let output_root = output_root.into();
    let mut diagnostics = Vec::new();
    if configuration.plugin_library_paths.is_empty() {
        let message = "source normalizer normalized-resource open skipped because no plugin paths were provided";
        diagnostics.push(runtime_source_normalizer_diagnostic(
            String::new(),
            None,
            PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
            message,
            PlayerPluginParticipation::Bypassed,
        ));
        return match configuration.mode {
            SourceNormalizerMode::RequireNormalized => Err(message.to_owned()),
            _ => Ok(None),
        };
    }

    let registry =
        PluginRegistry::inspect_source_normalizer_support(&configuration.plugin_library_paths);
    diagnostics.extend(
        registry
            .records()
            .iter()
            .map(|record| diagnostic_from_record(record, source_normalizer_participation(record))),
    );
    let Some(record) = registry.best_source_normalizer_resource() else {
        let message = format!(
            "source normalizer normalized-resource open skipped because no resource-output plugin is available: {}",
            mobile_source_normalizer_registry_notes(&registry)
        );
        diagnostics.push(runtime_source_normalizer_diagnostic(
            String::new(),
            None,
            PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
            message.clone(),
            PlayerPluginParticipation::Bypassed,
        ));
        return match configuration.mode {
            SourceNormalizerMode::RequireNormalized => Err(message),
            _ => Ok(None),
        };
    };

    match open_source_normalizer_resource_session(source, configuration, output_root, record) {
        Ok(mut opened) => {
            opened.diagnostics.splice(0..0, diagnostics);
            Ok(Some(opened))
        }
        Err(error) => {
            diagnostics.push(runtime_source_normalizer_diagnostic(
                record.path.display().to_string(),
                record.plugin_name.clone(),
                PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
                format!(
                    "source normalizer normalized-resource open failed; route=fallbackOriginal; error={error}"
                ),
                PlayerPluginParticipation::Bypassed,
            ));
            match configuration.mode {
                SourceNormalizerMode::RequireNormalized => Err(error),
                _ => Ok(None),
            }
        }
    }
}

pub fn mobile_source_normalizer_resource_open_json(
    handle: u64,
    opened: &MobileSourceNormalizerResourceOpen,
    playback_uri: Option<String>,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&MobileSourceNormalizerResourceWire::from_open(
        handle,
        opened,
        playback_uri,
    ))
}

pub fn mobile_source_normalizer_resource_status_json(
    handle: u64,
    opened: &MobileSourceNormalizerResourceOpen,
    playback_uri: Option<String>,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&MobileSourceNormalizerResourceWire::from_open(
        handle,
        opened,
        playback_uri,
    ))
}

pub fn source_normalizer_diagnostics(
    source: &MediaSource,
    configuration: &MobileSourceNormalizerConfiguration,
) -> Vec<PlayerPluginDiagnostic> {
    if configuration.mode == SourceNormalizerMode::Disabled
        && configuration.plugin_library_paths.is_empty()
    {
        return Vec::new();
    }

    if configuration.plugin_library_paths.is_empty() {
        return vec![runtime_source_normalizer_diagnostic(
            String::new(),
            None,
            PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
            "source normalizer mobile configuration is enabled, but no plugin paths were provided",
            PlayerPluginParticipation::Unknown,
        )];
    }

    let registry =
        PluginRegistry::inspect_source_normalizer_support(&configuration.plugin_library_paths);
    let mut diagnostics = registry
        .records()
        .iter()
        .map(|record| diagnostic_from_record(record, source_normalizer_participation(record)))
        .collect::<Vec<_>>();

    if configuration.mode == SourceNormalizerMode::PreferNormalized
        || configuration.mode == SourceNormalizerMode::RequireNormalized
    {
        let Some(record) = registry.best_source_normalizer_resource() else {
            diagnostics.push(runtime_source_normalizer_diagnostic(
                String::new(),
                None,
                PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
                "source normalizer resource probe skipped because no resource-output source normalizer plugin is available",
                PlayerPluginParticipation::Bypassed,
            ));
            return diagnostics;
        };

        diagnostics.push(probe_source_normalizer_resource(
            source,
            configuration,
            record,
        ));
        return diagnostics;
    }

    if configuration.mode != SourceNormalizerMode::PreflightOnly {
        return diagnostics;
    }

    let Some(record) = registry.best_source_normalizer_packet() else {
        diagnostics.push(runtime_source_normalizer_diagnostic(
            String::new(),
            None,
            PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
            "source normalizer preflight skipped because no packet-stream source normalizer plugin is available",
            PlayerPluginParticipation::Unknown,
        ));
        return diagnostics;
    };

    diagnostics.push(preflight_source_normalizer(source, configuration, record));
    diagnostics
}

pub fn frame_processor_diagnostics(
    configuration: &MobileFrameProcessorConfiguration,
) -> Vec<PlayerPluginDiagnostic> {
    if configuration.mode == FrameProcessorMode::Disabled
        && configuration.plugin_library_paths.is_empty()
    {
        return Vec::new();
    }

    if configuration.plugin_library_paths.is_empty() {
        return vec![runtime_frame_processor_diagnostic(
            String::new(),
            None,
            "frame processor mobile diagnostics are enabled, but no plugin paths were provided",
            PlayerPluginParticipation::Unknown,
        )];
    }

    PluginRegistry::inspect_frame_processor_support(&configuration.plugin_library_paths)
        .records()
        .iter()
        .map(|record| diagnostic_from_record(record, frame_processor_participation(record)))
        .collect()
}

fn preflight_source_normalizer(
    source: &MediaSource,
    configuration: &MobileSourceNormalizerConfiguration,
    record: &PluginDiagnosticRecord,
) -> PlayerPluginDiagnostic {
    let started = Instant::now();
    let path = record.path.display().to_string();
    let plugin = match LoadedDynamicPlugin::load(&record.path) {
        Ok(plugin) => plugin,
        Err(error) => {
            return runtime_source_normalizer_diagnostic(
                path,
                record.plugin_name.clone(),
                PlayerPluginDiagnosticStatus::LoadFailed,
                format!("source normalizer preflight load failed: {error}"),
                PlayerPluginParticipation::Bypassed,
            );
        }
    };
    let Some(factory) = plugin.source_normalizer_packet_plugin_factory() else {
        return runtime_source_normalizer_diagnostic(
            path,
            Some(plugin.plugin_name().to_owned()),
            PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
            format!(
                "{} is not a packet-stream source normalizer plugin",
                plugin.plugin_name()
            ),
            PlayerPluginParticipation::Bypassed,
        );
    };
    let runtime_profile = configuration.runtime_profile.clone().unwrap_or_default();
    let config = SourceNormalizerPacketSessionConfig {
        runtime_profile,
        input: source.uri().to_owned(),
        headers: Vec::new(),
        startup_timeout_ms: Some(SOURCE_NORMALIZER_STARTUP_TIMEOUT.as_millis() as u64),
        session_timeout_ms: Some(SOURCE_NORMALIZER_SESSION_TIMEOUT.as_millis() as u64),
        preferred_media_kind: SourceNormalizerPacketMediaKind::Video,
    };
    let mut session = match factory.open_packet_session(&config) {
        Ok(session) => session,
        Err(error) => {
            return runtime_source_normalizer_diagnostic(
                path,
                Some(factory.name().to_owned()),
                PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
                format!("source normalizer preflight open failed: {error}"),
                PlayerPluginParticipation::Bypassed,
            );
        }
    };
    let stream_info = session.stream_info();
    let close_message = match session.close() {
        Ok(()) => None,
        Err(error) => Some(format!("; close failed: {error}")),
    };
    let track_summary = stream_info
        .tracks
        .iter()
        .map(|track| format!("{}:{}", media_kind_label(track.media_kind), track.codec))
        .collect::<Vec<_>>();
    runtime_source_normalizer_diagnostic(
        path,
        stream_info
            .normalizer_name
            .clone()
            .or_else(|| Some(factory.name().to_owned())),
        PlayerPluginDiagnosticStatus::SourceNormalizerSupported,
        format!(
            "source normalizer preflight opened and closed packet session; profile={}; tracks={}; ready_ms={}{}",
            stream_info
                .runtime_profile
                .as_deref()
                .unwrap_or("auto-detected"),
            if track_summary.is_empty() {
                "none".to_owned()
            } else {
                track_summary.join(",")
            },
            started.elapsed().as_millis(),
            close_message.unwrap_or_default()
        ),
        PlayerPluginParticipation::Bypassed,
    )
}

fn probe_source_normalizer_resource(
    source: &MediaSource,
    configuration: &MobileSourceNormalizerConfiguration,
    record: &PluginDiagnosticRecord,
) -> PlayerPluginDiagnostic {
    let started = Instant::now();
    let path = record.path.display().to_string();
    let plugin = match LoadedDynamicPlugin::load(&record.path) {
        Ok(plugin) => plugin,
        Err(error) => {
            return runtime_source_normalizer_diagnostic(
                path,
                record.plugin_name.clone(),
                PlayerPluginDiagnosticStatus::LoadFailed,
                format!("source normalizer resource probe load failed: {error}"),
                PlayerPluginParticipation::Bypassed,
            );
        }
    };
    let Some(factory) = plugin.source_normalizer_resource_plugin_factory() else {
        return runtime_source_normalizer_diagnostic(
            path,
            Some(plugin.plugin_name().to_owned()),
            PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
            format!(
                "{} is not a resource-output source normalizer plugin",
                plugin.plugin_name()
            ),
            PlayerPluginParticipation::Bypassed,
        );
    };

    let runtime_profile = configuration.runtime_profile.clone().unwrap_or_default();
    let output_root = std::env::temp_dir()
        .join("vesper-source-normalizer-probe")
        .display()
        .to_string();
    let config = SourceNormalizerResourceSessionConfig {
        runtime_profile,
        input: source.uri().to_owned(),
        headers: Vec::new(),
        output_root,
        cache_policy: SourceNormalizerResourceCachePolicy::default(),
        preferred_route: preferred_resource_route_for_source(source),
        startup_timeout_ms: Some(SOURCE_NORMALIZER_STARTUP_TIMEOUT.as_millis() as u64),
        read_idle_timeout_ms: Some(SOURCE_NORMALIZER_SESSION_TIMEOUT.as_millis() as u64),
    };
    let mut session = match factory.open_resource_session(&config) {
        Ok(session) => session,
        Err(error) => {
            return runtime_source_normalizer_diagnostic(
                path,
                Some(factory.name().to_owned()),
                PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported,
                format!(
                    "source normalizer resource probe open failed; route=fallbackOriginal; error={error}"
                ),
                PlayerPluginParticipation::Bypassed,
            );
        }
    };
    let session_info = session.session_info();
    let poll_summary = match session.poll() {
        Ok(status) => {
            let route = status
                .info
                .as_ref()
                .map(|info| info.output_route.wire_name())
                .unwrap_or_else(|| session_info.output_route.wire_name());
            format!("state={:?}; route={route}", status.state)
        }
        Err(error) => format!("poll failed: {error}"),
    };
    let close_message = match session.close() {
        Ok(()) => None,
        Err(error) => Some(format!("; close failed: {error}")),
    };
    let participation = match session_info.output_route {
        SourceNormalizerOutputRoute::PacketStream => PlayerPluginParticipation::Bypassed,
        SourceNormalizerOutputRoute::Fmp4LocalStream
        | SourceNormalizerOutputRoute::HlsShortWindow => {
            if configuration.mode == SourceNormalizerMode::RequireNormalized {
                PlayerPluginParticipation::Selected
            } else {
                PlayerPluginParticipation::Bypassed
            }
        }
    };

    runtime_source_normalizer_diagnostic(
        path,
        session_info
            .normalizer_name
            .clone()
            .or_else(|| Some(factory.name().to_owned())),
        PlayerPluginDiagnosticStatus::SourceNormalizerSupported,
        format!(
            "source normalizer resource probe opened disk-backed session; route={}; profile={}; container={}; content_type={}; {}; ready_ms={}{}",
            session_info.output_route.wire_name(),
            session_info
                .runtime_profile
                .as_deref()
                .unwrap_or("auto-detected"),
            session_info.container,
            session_info
                .primary_content_type
                .as_deref()
                .unwrap_or("unknown"),
            poll_summary,
            started.elapsed().as_millis(),
            close_message.unwrap_or_default()
        ),
        participation,
    )
}

fn open_source_normalizer_resource_session(
    source: &MediaSource,
    configuration: &MobileSourceNormalizerConfiguration,
    output_root: String,
    record: &PluginDiagnosticRecord,
) -> Result<MobileSourceNormalizerResourceOpen, String> {
    let started = Instant::now();
    let path = record.path.display().to_string();
    let plugin = LoadedDynamicPlugin::load(&record.path)
        .map_err(|error| format!("source normalizer resource load failed: {error}"))?;
    let factory = plugin
        .source_normalizer_resource_plugin_factory()
        .ok_or_else(|| {
            format!(
                "{} is not a resource-output source normalizer plugin",
                plugin.plugin_name()
            )
        })?;
    let runtime_profile = configuration.runtime_profile.clone().unwrap_or_default();
    let config = SourceNormalizerResourceSessionConfig {
        runtime_profile,
        input: source.uri().to_owned(),
        headers: Vec::new(),
        output_root,
        cache_policy: SourceNormalizerResourceCachePolicy::default(),
        preferred_route: preferred_resource_route_for_source(source),
        startup_timeout_ms: Some(SOURCE_NORMALIZER_STARTUP_TIMEOUT.as_millis() as u64),
        read_idle_timeout_ms: Some(SOURCE_NORMALIZER_SESSION_TIMEOUT.as_millis() as u64),
    };
    let mut session = factory
        .open_resource_session(&config)
        .map_err(|error| format!("open_resource_session failed: {error}"))?;
    let mut status = wait_for_resource_session_ready(session.as_mut())?;
    let info = status
        .info
        .clone()
        .unwrap_or_else(|| session.session_info());
    let participation = match info.output_route {
        SourceNormalizerOutputRoute::PacketStream => PlayerPluginParticipation::Bypassed,
        SourceNormalizerOutputRoute::Fmp4LocalStream
        | SourceNormalizerOutputRoute::HlsShortWindow => PlayerPluginParticipation::Selected,
    };
    let diagnostic = runtime_source_normalizer_diagnostic(
        path.clone(),
        info.normalizer_name
            .clone()
            .or_else(|| Some(factory.name().to_owned())),
        PlayerPluginDiagnosticStatus::SourceNormalizerSupported,
        format!(
            "source normalizer normalized-resource opened; route={}; profile={}; container={}; content_type={}; disk_bytes={}; ready_ms={}",
            info.output_route.wire_name(),
            info.runtime_profile.as_deref().unwrap_or("auto-detected"),
            info.container,
            info.primary_content_type.as_deref().unwrap_or("unknown"),
            status
                .disk_bytes_used
                .or(info.disk_bytes_used)
                .map(|bytes| bytes.to_string())
                .unwrap_or_else(|| "unknown".to_owned()),
            started.elapsed().as_millis()
        ),
        participation,
    );
    status.info = Some(info.clone());
    Ok(MobileSourceNormalizerResourceOpen {
        plugin_name: info.normalizer_name.clone(),
        plugin_path: path,
        session,
        info,
        status,
        cache_policy: SourceNormalizerResourceCachePolicy::default(),
        diagnostics: vec![diagnostic],
    })
}

fn wait_for_resource_session_ready(
    session: &mut dyn SourceNormalizerResourceSession,
) -> Result<SourceNormalizerResourceSessionStatus, String> {
    let started = Instant::now();
    loop {
        let status = session
            .poll()
            .map_err(|error| format!("poll_resource_session failed: {error}"))?;
        match status.state {
            SourceNormalizerResourceSessionState::Ready
            | SourceNormalizerResourceSessionState::Completed => return Ok(status),
            SourceNormalizerResourceSessionState::Failed => {
                return Err(status
                    .message
                    .unwrap_or_else(|| "source normalizer resource session failed".to_owned()));
            }
            SourceNormalizerResourceSessionState::Cancelled => {
                return Err(status.message.unwrap_or_else(|| {
                    "source normalizer resource session was cancelled".to_owned()
                }));
            }
            SourceNormalizerResourceSessionState::Starting
            | SourceNormalizerResourceSessionState::Running => {
                if resource_status_has_primary_bytes(&status) {
                    return Ok(status);
                }
                if started.elapsed() >= SOURCE_NORMALIZER_RESOURCE_READY_TIMEOUT {
                    return Err(status.message.unwrap_or_else(|| {
                        "source normalizer resource did not produce a readable primary resource before startup timeout".to_owned()
                    }));
                }
                std::thread::sleep(SOURCE_NORMALIZER_RESOURCE_POLL_INTERVAL);
            }
        }
    }
}

fn resource_status_has_primary_bytes(status: &SourceNormalizerResourceSessionStatus) -> bool {
    let Some(info) = status.info.as_ref() else {
        return false;
    };
    let Some(primary_path) = info.primary_resource_path.as_deref() else {
        return false;
    };
    if primary_path.is_empty() {
        return false;
    }
    match info.output_route {
        SourceNormalizerOutputRoute::Fmp4LocalStream => {
            let primary_len = std::fs::metadata(primary_path)
                .ok()
                .map(|metadata| metadata.len())
                .unwrap_or_default();
            primary_len > 32 && file_contains_box_marker(primary_path, &[b"moov", b"moof"])
        }
        SourceNormalizerOutputRoute::HlsShortWindow => {
            let playlist_ready = std::fs::read_to_string(primary_path)
                .map(|playlist| {
                    playlist.contains("#EXTM3U")
                        && (playlist.contains("#EXTINF") || playlist.contains("#EXT-X-MAP"))
                })
                .unwrap_or(false);
            if !playlist_ready {
                return false;
            }
            info.resources.iter().any(|resource| {
                resource.path != primary_path
                    && resource.byte_length.unwrap_or_default() > 0
                    && (resource.role == "segment"
                        || resource
                            .content_type
                            .as_deref()
                            .map(|content_type| content_type.starts_with("video/"))
                            .unwrap_or(false))
            })
        }
        SourceNormalizerOutputRoute::PacketStream => false,
    }
}

fn file_contains_box_marker(path: &str, markers: &[&[u8; 4]]) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    let max_scan = bytes.len().min(1024 * 1024);
    bytes[..max_scan]
        .windows(4)
        .any(|window| markers.iter().any(|marker| window == marker.as_slice()))
}

fn diagnostic_from_record(
    record: &PluginDiagnosticRecord,
    participation: PlayerPluginParticipation,
) -> PlayerPluginDiagnostic {
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
                PlayerPluginDiagnosticStatus::SourceNormalizerSupported
            }
            PluginDiagnosticStatus::SourceNormalizerUnsupported => {
                PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported
            }
        },
        message: record.message.clone(),
        capability: record
            .capability_summary
            .as_ref()
            .map(capability_summary_from_loader),
        participation,
    }
}

fn capability_summary_from_loader(
    summary: &PluginCapabilitySummary,
) -> PlayerPluginCapabilitySummary {
    match summary {
        PluginCapabilitySummary::Decoder(summary) => PlayerPluginCapabilitySummary::Decoder(
            player_runtime::PlayerPluginDecoderCapabilitySummary {
                codecs: summary
                    .typed_codecs
                    .iter()
                    .map(|codec| player_runtime::PlayerPluginCodecCapability {
                        media_kind: match codec.media_kind {
                            player_plugin::DecoderMediaKind::Video => "video",
                            player_plugin::DecoderMediaKind::Audio => "audio",
                        }
                        .to_owned(),
                        codec: codec.codec.clone(),
                    })
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
            },
        ),
        PluginCapabilitySummary::FrameProcessor(summary) => {
            PlayerPluginCapabilitySummary::FrameProcessor(frame_processor_summary_from_loader(
                summary,
            ))
        }
        PluginCapabilitySummary::SourceNormalizerPacket(summary) => {
            PlayerPluginCapabilitySummary::SourceNormalizer(source_normalizer_summary_from_loader(
                summary,
            ))
        }
        PluginCapabilitySummary::SourceNormalizerResource(summary) => {
            PlayerPluginCapabilitySummary::SourceNormalizer(
                source_normalizer_resource_summary_from_loader(summary),
            )
        }
    }
}

fn frame_processor_summary_from_loader(
    summary: &FrameProcessorPluginCapabilitySummary,
) -> PlayerPluginFrameProcessorCapabilitySummary {
    PlayerPluginFrameProcessorCapabilitySummary {
        accepted_input_handle_kinds: summary
            .accepted_input_handle_kinds
            .iter()
            .map(|kind| format!("{kind:?}"))
            .collect(),
        output_handle_kinds: summary
            .output_handle_kinds
            .iter()
            .map(|kind| format!("{kind:?}"))
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

fn source_normalizer_summary_from_loader(
    summary: &SourceNormalizerPacketPluginCapabilitySummary,
) -> PlayerPluginSourceNormalizerCapabilitySummary {
    PlayerPluginSourceNormalizerCapabilitySummary {
        supported_runtime_profiles: summary.supported_runtime_profiles.clone(),
        supported_output_routes: vec![
            SourceNormalizerOutputRoute::PacketStream
                .wire_name()
                .to_owned(),
        ],
        max_level: normalize_level_label(summary.max_level).to_owned(),
        media_kinds: summary
            .media_kinds
            .iter()
            .map(|kind| media_kind_label(*kind).to_owned())
            .collect(),
        codecs: summary.codecs.clone(),
        bitstream_formats: summary
            .bitstream_formats
            .iter()
            .map(|format| bitstream_format_label(format).to_owned())
            .collect(),
        supports_seek: summary.supports_seek,
        supports_flush: summary.supports_flush,
        supports_growing_resources: false,
        supports_range_reads: false,
        supports_cancel: false,
        content_types: Vec::new(),
        required_libraries: summary.required_capabilities.libraries.clone(),
        required_demuxers: summary.required_capabilities.demuxers.clone(),
        required_muxers: summary.required_capabilities.muxers.clone(),
        required_protocols: summary.required_capabilities.protocols.clone(),
        required_parsers: summary.required_capabilities.parsers.clone(),
        required_bitstream_filters: summary.required_capabilities.bitstream_filters.clone(),
        required_tls: summary.required_capabilities.tls.clone(),
        requires_network: summary.required_capabilities.network,
        session_read_buffer_bytes: None,
        manifest_snapshot_bytes: None,
        session_disk_soft_cap_bytes: None,
        global_disk_soft_cap_bytes: None,
        max_sessions: summary.max_sessions,
    }
}

fn source_normalizer_resource_summary_from_loader(
    summary: &SourceNormalizerResourcePluginCapabilitySummary,
) -> PlayerPluginSourceNormalizerCapabilitySummary {
    PlayerPluginSourceNormalizerCapabilitySummary {
        supported_runtime_profiles: summary.supported_runtime_profiles.clone(),
        supported_output_routes: summary.supported_output_routes.clone(),
        max_level: normalize_level_label(summary.max_level).to_owned(),
        media_kinds: Vec::new(),
        codecs: Vec::new(),
        bitstream_formats: Vec::new(),
        supports_seek: false,
        supports_flush: false,
        supports_growing_resources: summary.supports_growing_resources,
        supports_range_reads: summary.supports_range_reads,
        supports_cancel: summary.supports_cancel,
        content_types: summary.content_types.clone(),
        required_libraries: summary.required_capabilities.libraries.clone(),
        required_demuxers: summary.required_capabilities.demuxers.clone(),
        required_muxers: summary.required_capabilities.muxers.clone(),
        required_protocols: summary.required_capabilities.protocols.clone(),
        required_parsers: summary.required_capabilities.parsers.clone(),
        required_bitstream_filters: summary.required_capabilities.bitstream_filters.clone(),
        required_tls: summary.required_capabilities.tls.clone(),
        requires_network: summary.required_capabilities.network,
        session_read_buffer_bytes: Some(summary.cache_policy.session_read_buffer_bytes),
        manifest_snapshot_bytes: Some(summary.cache_policy.manifest_snapshot_bytes),
        session_disk_soft_cap_bytes: Some(summary.cache_policy.session_disk_soft_cap_bytes),
        global_disk_soft_cap_bytes: Some(summary.cache_policy.global_disk_soft_cap_bytes),
        max_sessions: summary.max_sessions,
    }
}

fn source_normalizer_participation(record: &PluginDiagnosticRecord) -> PlayerPluginParticipation {
    if record.status == PluginDiagnosticStatus::SourceNormalizerSupported {
        PlayerPluginParticipation::Available
    } else {
        PlayerPluginParticipation::Unknown
    }
}

fn frame_processor_participation(record: &PluginDiagnosticRecord) -> PlayerPluginParticipation {
    if record.status == PluginDiagnosticStatus::FrameProcessorSupported {
        PlayerPluginParticipation::Available
    } else {
        PlayerPluginParticipation::Unknown
    }
}

fn runtime_source_normalizer_diagnostic(
    path: String,
    plugin_name: Option<String>,
    status: PlayerPluginDiagnosticStatus,
    message: impl Into<String>,
    participation: PlayerPluginParticipation,
) -> PlayerPluginDiagnostic {
    PlayerPluginDiagnostic {
        path,
        plugin_name,
        plugin_kind: Some("source_normalizer".to_owned()),
        status,
        message: Some(message.into()),
        capability: None,
        participation,
    }
}

fn runtime_frame_processor_diagnostic(
    path: String,
    plugin_name: Option<String>,
    message: impl Into<String>,
    participation: PlayerPluginParticipation,
) -> PlayerPluginDiagnostic {
    PlayerPluginDiagnostic {
        path,
        plugin_name,
        plugin_kind: Some("frame_processor".to_owned()),
        status: PlayerPluginDiagnosticStatus::FrameProcessorUnsupported,
        message: Some(message.into()),
        capability: None,
        participation,
    }
}

fn plugin_kind_label(kind: player_plugin::VesperPluginKind) -> &'static str {
    match kind {
        player_plugin::VesperPluginKind::PostDownloadProcessor => "post_download_processor",
        player_plugin::VesperPluginKind::PipelineEventHook => "pipeline_event_hook",
        player_plugin::VesperPluginKind::Decoder => "decoder",
        player_plugin::VesperPluginKind::BenchmarkSink => "benchmark_sink",
        player_plugin::VesperPluginKind::FrameProcessor => "frame_processor",
        player_plugin::VesperPluginKind::SourceNormalizer => "source_normalizer",
    }
}

fn normalize_level_label(level: SourceNormalizerNormalizeLevel) -> &'static str {
    match level {
        SourceNormalizerNormalizeLevel::RemuxOnly => "remux_only",
        SourceNormalizerNormalizeLevel::PacketRepair => "packet_repair",
    }
}

fn media_kind_label(kind: SourceNormalizerPacketMediaKind) -> &'static str {
    match kind {
        SourceNormalizerPacketMediaKind::Video => "video",
        SourceNormalizerPacketMediaKind::Audio => "audio",
        SourceNormalizerPacketMediaKind::Subtitle => "subtitle",
    }
}

fn preferred_resource_route_for_source(
    source: &MediaSource,
) -> Option<SourceNormalizerOutputRoute> {
    match source.protocol() {
        player_runtime::MediaSourceProtocol::Hls | player_runtime::MediaSourceProtocol::Dash => {
            Some(SourceNormalizerOutputRoute::HlsShortWindow)
        }
        _ => Some(SourceNormalizerOutputRoute::Fmp4LocalStream),
    }
}

fn native_first_source_normalizer_bypass(source: &MediaSource) -> bool {
    matches!(
        source.protocol(),
        player_runtime::MediaSourceProtocol::Hls | player_runtime::MediaSourceProtocol::Dash
    )
}

fn mobile_source_normalizer_registry_notes(registry: &PluginRegistry) -> String {
    let records = registry.records();
    if records.is_empty() {
        return "no plugin records".to_owned();
    }
    records
        .iter()
        .map(|record| {
            format!(
                "{}:{}",
                record.path.display(),
                record
                    .message
                    .as_deref()
                    .unwrap_or_else(|| status_note(record.status))
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn status_note(status: PluginDiagnosticStatus) -> &'static str {
    match status {
        PluginDiagnosticStatus::Loaded => "loaded",
        PluginDiagnosticStatus::LoadFailed => "load failed",
        PluginDiagnosticStatus::UnsupportedKind => "unsupported kind",
        PluginDiagnosticStatus::DecoderSupported => "decoder supported",
        PluginDiagnosticStatus::DecoderUnsupported => "decoder unsupported",
        PluginDiagnosticStatus::FrameProcessorSupported => "frame processor supported",
        PluginDiagnosticStatus::FrameProcessorUnsupported => "frame processor unsupported",
        PluginDiagnosticStatus::SourceNormalizerSupported => "source normalizer supported",
        PluginDiagnosticStatus::SourceNormalizerUnsupported => "source normalizer unsupported",
    }
}

fn bitstream_format_label(format: &DecoderBitstreamFormat) -> &'static str {
    match format {
        DecoderBitstreamFormat::AnnexB => "annex_b",
        DecoderBitstreamFormat::Avcc => "avcc",
        DecoderBitstreamFormat::Hvcc => "hvcc",
        DecoderBitstreamFormat::Unknown(_) => "unknown",
    }
}

pub fn mobile_plugin_diagnostics_json(
    source: &MediaSource,
    source_normalizer: &MobileSourceNormalizerConfiguration,
    frame_processor: &MobileFrameProcessorConfiguration,
) -> Result<String, serde_json::Error> {
    let mut diagnostics = source_normalizer_diagnostics(source, source_normalizer);
    diagnostics.extend(frame_processor_diagnostics(frame_processor));
    serde_json::to_string(
        &diagnostics
            .iter()
            .map(MobilePluginDiagnosticWire::from)
            .collect::<Vec<_>>(),
    )
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobilePluginDiagnosticWire<'a> {
    path: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    plugin_name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    plugin_kind: Option<&'a str>,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    capability: Option<WirePluginCapability<'a>>,
    participation: &'static str,
}

impl MobileSourceNormalizerResourceWire {
    pub fn from_open(
        handle: u64,
        opened: &MobileSourceNormalizerResourceOpen,
        playback_uri: Option<String>,
    ) -> Self {
        let info = opened.status.info.as_ref().unwrap_or(&opened.info);
        Self {
            handle,
            plugin_name: opened.plugin_name.clone(),
            plugin_path: opened.plugin_path.clone(),
            output_route: info.output_route.wire_name().to_owned(),
            selected_profile: info.runtime_profile.clone(),
            container: info.container.clone(),
            primary_resource_path: info.primary_resource_path.clone(),
            primary_content_type: info.primary_content_type.clone(),
            playback_uri,
            resources: info
                .resources
                .iter()
                .map(MobileSourceNormalizerResourceInfoWire::from)
                .collect(),
            state: resource_state_wire_name(opened.status.state).to_owned(),
            message: opened.status.message.clone(),
            disk_bytes_used: opened.status.disk_bytes_used.or(info.disk_bytes_used),
            cache_policy: MobileSourceNormalizerCachePolicyWire::from(&opened.cache_policy),
            diagnostics: opened
                .diagnostics
                .iter()
                .map(MobilePluginDiagnosticOwnedWire::from)
                .collect(),
        }
    }
}

impl From<&SourceNormalizerResourceCachePolicy> for MobileSourceNormalizerCachePolicyWire {
    fn from(policy: &SourceNormalizerResourceCachePolicy) -> Self {
        Self {
            session_read_buffer_bytes: policy.session_read_buffer_bytes,
            manifest_snapshot_bytes: policy.manifest_snapshot_bytes,
            session_disk_soft_cap_bytes: policy.session_disk_soft_cap_bytes,
            global_disk_soft_cap_bytes: policy.global_disk_soft_cap_bytes,
        }
    }
}

impl From<&player_plugin::SourceNormalizerResourceInfo> for MobileSourceNormalizerResourceInfoWire {
    fn from(value: &player_plugin::SourceNormalizerResourceInfo) -> Self {
        Self {
            role: value.role.clone(),
            path: value.path.clone(),
            content_type: value.content_type.clone(),
            byte_length: value.byte_length,
            growing: value.growing,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobilePluginDiagnosticOwnedWire {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    plugin_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    plugin_kind: Option<String>,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    participation: &'static str,
}

impl From<&PlayerPluginDiagnostic> for MobilePluginDiagnosticOwnedWire {
    fn from(value: &PlayerPluginDiagnostic) -> Self {
        Self {
            path: value.path.clone(),
            plugin_name: value.plugin_name.clone(),
            plugin_kind: value.plugin_kind.clone(),
            status: status_wire_name(value.status),
            message: value.message.clone(),
            participation: participation_wire_name(value.participation),
        }
    }
}

impl<'a> From<&'a PlayerPluginDiagnostic> for MobilePluginDiagnosticWire<'a> {
    fn from(value: &'a PlayerPluginDiagnostic) -> Self {
        Self {
            path: value.path.as_str(),
            plugin_name: value.plugin_name.as_deref(),
            plugin_kind: value.plugin_kind.as_deref(),
            status: status_wire_name(value.status),
            message: value.message.as_deref(),
            capability: value.capability.as_ref().map(WirePluginCapability::from),
            participation: participation_wire_name(value.participation),
        }
    }
}

pub fn resource_state_wire_name(state: SourceNormalizerResourceSessionState) -> &'static str {
    match state {
        SourceNormalizerResourceSessionState::Starting => "starting",
        SourceNormalizerResourceSessionState::Ready => "ready",
        SourceNormalizerResourceSessionState::Running => "running",
        SourceNormalizerResourceSessionState::Completed => "completed",
        SourceNormalizerResourceSessionState::Failed => "failed",
        SourceNormalizerResourceSessionState::Cancelled => "cancelled",
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WirePluginCapability<'a> {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    decoder: Option<WireDecoderCapability<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frame_processor: Option<WireFrameProcessorCapability<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_normalizer: Option<WireSourceNormalizerCapability<'a>>,
}

impl<'a> From<&'a PlayerPluginCapabilitySummary> for WirePluginCapability<'a> {
    fn from(value: &'a PlayerPluginCapabilitySummary) -> Self {
        match value {
            PlayerPluginCapabilitySummary::Decoder(summary) => Self {
                kind: "decoder",
                decoder: Some(WireDecoderCapability::from(summary)),
                frame_processor: None,
                source_normalizer: None,
            },
            PlayerPluginCapabilitySummary::FrameProcessor(summary) => Self {
                kind: "frameProcessor",
                decoder: None,
                frame_processor: Some(WireFrameProcessorCapability::from(summary)),
                source_normalizer: None,
            },
            PlayerPluginCapabilitySummary::SourceNormalizer(summary) => Self {
                kind: "sourceNormalizer",
                decoder: None,
                frame_processor: None,
                source_normalizer: Some(WireSourceNormalizerCapability::from(summary)),
            },
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WirePluginCodecCapability<'a> {
    media_kind: &'a str,
    codec: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireDecoderCapability<'a> {
    codecs: Vec<WirePluginCodecCapability<'a>>,
    legacy_codecs: &'a [String],
    supports_native_frame_output: bool,
    supports_hardware_decode: bool,
    supports_cpu_video_frames: bool,
    supports_audio_frames: bool,
    supports_gpu_handles: bool,
    supports_flush: bool,
    supports_drain: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_sessions: Option<u32>,
}

impl<'a> From<&'a player_runtime::PlayerPluginDecoderCapabilitySummary>
    for WireDecoderCapability<'a>
{
    fn from(value: &'a player_runtime::PlayerPluginDecoderCapabilitySummary) -> Self {
        Self {
            codecs: value
                .codecs
                .iter()
                .map(|codec| WirePluginCodecCapability {
                    media_kind: codec.media_kind.as_str(),
                    codec: codec.codec.as_str(),
                })
                .collect(),
            legacy_codecs: &value.legacy_codecs,
            supports_native_frame_output: value.supports_native_frame_output,
            supports_hardware_decode: value.supports_hardware_decode,
            supports_cpu_video_frames: value.supports_cpu_video_frames,
            supports_audio_frames: value.supports_audio_frames,
            supports_gpu_handles: value.supports_gpu_handles,
            supports_flush: value.supports_flush,
            supports_drain: value.supports_drain,
            max_sessions: value.max_sessions,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireFrameProcessorCapability<'a> {
    accepted_input_handle_kinds: &'a [String],
    output_handle_kinds: &'a [String],
    supports_video_frames: bool,
    supports_in_place_passthrough: bool,
    preserves_dimensions: bool,
    may_change_dimensions: bool,
    preserves_color_metadata: bool,
    preserves_hdr_metadata: bool,
    supports_flush: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_sessions: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_in_flight_frames: Option<u32>,
}

impl<'a> From<&'a PlayerPluginFrameProcessorCapabilitySummary>
    for WireFrameProcessorCapability<'a>
{
    fn from(value: &'a PlayerPluginFrameProcessorCapabilitySummary) -> Self {
        Self {
            accepted_input_handle_kinds: &value.accepted_input_handle_kinds,
            output_handle_kinds: &value.output_handle_kinds,
            supports_video_frames: value.supports_video_frames,
            supports_in_place_passthrough: value.supports_in_place_passthrough,
            preserves_dimensions: value.preserves_dimensions,
            may_change_dimensions: value.may_change_dimensions,
            preserves_color_metadata: value.preserves_color_metadata,
            preserves_hdr_metadata: value.preserves_hdr_metadata,
            supports_flush: value.supports_flush,
            max_sessions: value.max_sessions,
            max_in_flight_frames: value.max_in_flight_frames,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireSourceNormalizerCapability<'a> {
    supported_runtime_profiles: &'a [String],
    supported_output_routes: &'a [String],
    max_level: &'a str,
    media_kinds: &'a [String],
    codecs: &'a [String],
    bitstream_formats: &'a [String],
    supports_seek: bool,
    supports_flush: bool,
    supports_growing_resources: bool,
    supports_range_reads: bool,
    supports_cancel: bool,
    content_types: &'a [String],
    required_libraries: &'a [String],
    required_demuxers: &'a [String],
    required_muxers: &'a [String],
    required_protocols: &'a [String],
    required_parsers: &'a [String],
    required_bitstream_filters: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    required_tls: Option<&'a str>,
    requires_network: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_read_buffer_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    manifest_snapshot_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_disk_soft_cap_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    global_disk_soft_cap_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_sessions: Option<u32>,
}

impl<'a> From<&'a PlayerPluginSourceNormalizerCapabilitySummary>
    for WireSourceNormalizerCapability<'a>
{
    fn from(value: &'a PlayerPluginSourceNormalizerCapabilitySummary) -> Self {
        Self {
            supported_runtime_profiles: &value.supported_runtime_profiles,
            supported_output_routes: &value.supported_output_routes,
            max_level: value.max_level.as_str(),
            media_kinds: &value.media_kinds,
            codecs: &value.codecs,
            bitstream_formats: &value.bitstream_formats,
            supports_seek: value.supports_seek,
            supports_flush: value.supports_flush,
            supports_growing_resources: value.supports_growing_resources,
            supports_range_reads: value.supports_range_reads,
            supports_cancel: value.supports_cancel,
            content_types: &value.content_types,
            required_libraries: &value.required_libraries,
            required_demuxers: &value.required_demuxers,
            required_muxers: &value.required_muxers,
            required_protocols: &value.required_protocols,
            required_parsers: &value.required_parsers,
            required_bitstream_filters: &value.required_bitstream_filters,
            required_tls: value.required_tls.as_deref(),
            requires_network: value.requires_network,
            session_read_buffer_bytes: value.session_read_buffer_bytes,
            manifest_snapshot_bytes: value.manifest_snapshot_bytes,
            session_disk_soft_cap_bytes: value.session_disk_soft_cap_bytes,
            global_disk_soft_cap_bytes: value.global_disk_soft_cap_bytes,
            max_sessions: value.max_sessions,
        }
    }
}

fn status_wire_name(status: PlayerPluginDiagnosticStatus) -> &'static str {
    match status {
        PlayerPluginDiagnosticStatus::Loaded => "loaded",
        PlayerPluginDiagnosticStatus::LoadFailed => "loadFailed",
        PlayerPluginDiagnosticStatus::UnsupportedKind => "unsupportedKind",
        PlayerPluginDiagnosticStatus::DecoderSupported => "decoderSupported",
        PlayerPluginDiagnosticStatus::DecoderUnsupported => "decoderUnsupported",
        PlayerPluginDiagnosticStatus::FrameProcessorSupported => "frameProcessorSupported",
        PlayerPluginDiagnosticStatus::FrameProcessorUnsupported => "frameProcessorUnsupported",
        PlayerPluginDiagnosticStatus::SourceNormalizerSupported => "sourceNormalizerSupported",
        PlayerPluginDiagnosticStatus::SourceNormalizerUnsupported => "sourceNormalizerUnsupported",
    }
}

fn participation_wire_name(participation: PlayerPluginParticipation) -> &'static str {
    match participation {
        PlayerPluginParticipation::Unknown => "unknown",
        PlayerPluginParticipation::Available => "available",
        PlayerPluginParticipation::Selected => "selected",
        PlayerPluginParticipation::Participated => "participated",
        PlayerPluginParticipation::Bypassed => "bypassed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_configs_emit_no_diagnostics() {
        let diagnostics = apply_mobile_plugin_diagnostics(
            PlayerRuntimeStartup {
                ffmpeg_initialized: false,
                audio_output: None,
                decoded_audio: None,
                video_decode: None,
                plugin_diagnostics: Vec::new(),
            },
            &MediaSource::new("placeholder.mp4"),
            &MobilePluginConfiguration::default(),
        )
        .plugin_diagnostics;

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn source_normalizer_missing_paths_are_non_blocking() {
        let diagnostics = source_normalizer_diagnostics(
            &MediaSource::new("placeholder.mp4"),
            &MobileSourceNormalizerConfiguration {
                mode: SourceNormalizerMode::PreflightOnly,
                ..MobileSourceNormalizerConfiguration::default()
            },
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].plugin_kind.as_deref(),
            Some("source_normalizer")
        );
        assert_eq!(
            diagnostics[0].participation,
            PlayerPluginParticipation::Unknown
        );
    }

    #[test]
    fn frame_processor_missing_paths_are_diagnostic_only() {
        let diagnostics = frame_processor_diagnostics(&MobileFrameProcessorConfiguration {
            mode: FrameProcessorMode::DiagnosticsOnly,
            ..MobileFrameProcessorConfiguration::default()
        });

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].plugin_kind.as_deref(),
            Some("frame_processor")
        );
        assert_ne!(
            diagnostics[0].participation,
            PlayerPluginParticipation::Participated
        );
    }

    #[test]
    fn diagnostics_json_uses_shared_flutter_wire_names() {
        let json = mobile_plugin_diagnostics_json(
            &MediaSource::new("placeholder.mp4"),
            &MobileSourceNormalizerConfiguration {
                mode: SourceNormalizerMode::DiagnosticsOnly,
                ..MobileSourceNormalizerConfiguration::default()
            },
            &MobileFrameProcessorConfiguration {
                mode: FrameProcessorMode::DiagnosticsOnly,
                ..MobileFrameProcessorConfiguration::default()
            },
        )
        .expect("serialize diagnostics");

        assert!(json.contains("sourceNormalizerUnsupported"));
        assert!(json.contains("frameProcessorUnsupported"));
        assert!(json.contains("participation"));
    }

    #[test]
    fn prefer_normalized_native_first_bypasses_standard_adaptive_sources() {
        let opened = open_mobile_source_normalizer_resource(
            &MediaSource::new("https://cdn.example.test/master.m3u8"),
            &MobileSourceNormalizerConfiguration {
                mode: SourceNormalizerMode::PreferNormalized,
                plugin_library_paths: vec![PathBuf::from("/missing/source-normalizer.so")],
                runtime_profile: None,
            },
            std::env::temp_dir().display().to_string(),
            MobileSourceNormalizerRouteDecision::NativeFirst,
        )
        .expect("prefer native-first bypass should not error");

        assert!(opened.is_none());
    }

    #[test]
    fn prefer_normalized_native_first_bypasses_standard_dash_sources() {
        let opened = open_mobile_source_normalizer_resource(
            &MediaSource::new("https://cdn.example.test/manifest.mpd"),
            &MobileSourceNormalizerConfiguration {
                mode: SourceNormalizerMode::PreferNormalized,
                plugin_library_paths: vec![PathBuf::from("/missing/source-normalizer.so")],
                runtime_profile: Some("generic-fallback".to_owned()),
            },
            std::env::temp_dir().display().to_string(),
            MobileSourceNormalizerRouteDecision::NativeFirst,
        )
        .expect("prefer native-first DASH bypass should not error");

        assert!(
            opened.is_none(),
            "standard DASH stays native-first; generic fallback must not force normalization"
        );
    }

    #[test]
    fn require_normalized_errors_when_plugin_paths_are_missing() {
        let result = open_mobile_source_normalizer_resource(
            &MediaSource::new("file:///tmp/input.flv"),
            &MobileSourceNormalizerConfiguration {
                mode: SourceNormalizerMode::RequireNormalized,
                ..MobileSourceNormalizerConfiguration::default()
            },
            std::env::temp_dir().display().to_string(),
            MobileSourceNormalizerRouteDecision::Force,
        );
        let Err(error) = result else {
            panic!("requireNormalized must fail when no plugin is available");
        };

        assert!(error.contains("no plugin paths"));
    }
}
