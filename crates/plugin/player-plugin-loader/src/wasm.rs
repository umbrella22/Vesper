use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use player_plugin::{BenchmarkSink, PipelineEventHook, PluginReference, PluginTransport};
use player_plugin_abi::{BENCHMARK_SINK_INTERFACE_ID, PIPELINE_EVENT_HOOK_INTERFACE_ID};
use player_plugin_wasm_host::{
    MAX_WASM_PLUGIN_COMPONENT_BYTES, WASM_PLUGIN_WIT_INTERFACE_MAJOR,
    WASM_PLUGIN_WIT_INTERFACE_MINOR, WasmBenchmarkSinkAdapter, WasmPipelineEventHookAdapter,
    WasmPluginHostError, WasmPluginRuntime,
};
use thiserror::Error;

use crate::{
    PluginInterfaceMetadata, PluginInterfaceRecord, PluginInterfaceState, PluginSelectionError,
};

/// One interface declared by a host-verified WASM plugin catalog entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmPluginInterfaceDeclaration {
    interface_id: [u8; 16],
    major: u16,
    minor: u16,
    instance_id: String,
}

impl WasmPluginInterfaceDeclaration {
    pub fn new(
        interface_id: [u8; 16],
        major: u16,
        minor: u16,
        instance_id: impl Into<String>,
    ) -> Self {
        Self {
            interface_id,
            major,
            minor,
            instance_id: instance_id.into(),
        }
    }

    pub fn pipeline_event_hook(instance_id: impl Into<String>) -> Self {
        Self::new(
            PIPELINE_EVENT_HOOK_INTERFACE_ID.0,
            WASM_PLUGIN_WIT_INTERFACE_MAJOR,
            WASM_PLUGIN_WIT_INTERFACE_MINOR,
            instance_id,
        )
    }

    pub fn benchmark_sink(instance_id: impl Into<String>) -> Self {
        Self::new(
            BENCHMARK_SINK_INTERFACE_ID.0,
            WASM_PLUGIN_WIT_INTERFACE_MAJOR,
            WASM_PLUGIN_WIT_INTERFACE_MINOR,
            instance_id,
        )
    }

    pub const fn interface_id(&self) -> [u8; 16] {
        self.interface_id
    }

    pub const fn major(&self) -> u16 {
        self.major
    }

    pub const fn minor(&self) -> u16 {
        self.minor
    }

    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }
}

/// One verified WASM component supplied by a host-owned plugin catalog.
///
/// The component path remains an internal locator. Plugin selection is always
/// performed with a [`PluginReference`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmPluginArtifact {
    plugin_id: String,
    path: PathBuf,
    interfaces: Vec<WasmPluginInterfaceDeclaration>,
}

impl WasmPluginArtifact {
    pub fn new(
        plugin_id: impl Into<String>,
        path: impl Into<PathBuf>,
        interfaces: impl IntoIterator<Item = WasmPluginInterfaceDeclaration>,
    ) -> Result<Self, WasmPluginArtifactError> {
        let plugin_id = plugin_id.into();
        PluginReference::new(plugin_id.clone(), None, PluginTransport::Wasm)
            .map_err(|_| WasmPluginArtifactError::InvalidPluginId)?;

        let interfaces = interfaces.into_iter().collect::<Vec<_>>();
        if interfaces.is_empty() {
            return Err(WasmPluginArtifactError::MissingInterfaces);
        }

        let mut identities = BTreeSet::new();
        for interface in &interfaces {
            PluginReference::new(
                plugin_id.clone(),
                Some(interface.instance_id.clone()),
                PluginTransport::Wasm,
            )
            .map_err(|_| WasmPluginArtifactError::InvalidInstanceId {
                instance_id: interface.instance_id.clone(),
            })?;
            if !is_supported_interface(interface.interface_id) {
                return Err(WasmPluginArtifactError::UnsupportedInterface {
                    interface_id: interface.interface_id,
                });
            }
            if interface.major != WASM_PLUGIN_WIT_INTERFACE_MAJOR
                || interface.minor != WASM_PLUGIN_WIT_INTERFACE_MINOR
            {
                return Err(WasmPluginArtifactError::UnsupportedInterfaceVersion {
                    interface_id: interface.interface_id,
                    major: interface.major,
                    minor: interface.minor,
                    supported_major: WASM_PLUGIN_WIT_INTERFACE_MAJOR,
                    supported_minor: WASM_PLUGIN_WIT_INTERFACE_MINOR,
                });
            }
            let identity = (interface.interface_id, interface.instance_id.clone());
            if !identities.insert(identity) {
                return Err(WasmPluginArtifactError::DuplicateInterfaceIdentity {
                    interface_id: interface.interface_id,
                    instance_id: interface.instance_id.clone(),
                });
            }
        }

        Ok(Self {
            plugin_id,
            path: path.into(),
            interfaces,
        })
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn interfaces(&self) -> &[WasmPluginInterfaceDeclaration] {
        &self.interfaces
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WasmPluginArtifactError {
    #[error("plugin_id must be a valid reverse-DNS identity")]
    InvalidPluginId,
    #[error("WASM plugin artifact must declare at least one interface")]
    MissingInterfaces,
    #[error("capability instance `{instance_id}` must be a valid reverse-DNS identity")]
    InvalidInstanceId { instance_id: String },
    #[error("WASM plugins cannot implement interface {interface_id:?}")]
    UnsupportedInterface { interface_id: [u8; 16] },
    #[error(
        "WASM interface {interface_id:?} declares {major}.{minor}, but the host supports WIT {supported_major}.{supported_minor}"
    )]
    UnsupportedInterfaceVersion {
        interface_id: [u8; 16],
        major: u16,
        minor: u16,
        supported_major: u16,
        supported_minor: u16,
    },
    #[error("duplicate WASM interface identity {interface_id:?}:{instance_id}")]
    DuplicateInterfaceIdentity {
        interface_id: [u8; 16],
        instance_id: String,
    },
}

#[derive(Debug, Error)]
pub enum WasmPluginLoadError {
    #[error("failed to open WASM component `{path}`: {source}")]
    Open {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read WASM component `{path}`: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("WASM component `{path}` exceeds the {limit}-byte host limit")]
    ComponentTooLarge { path: String, limit: usize },
    #[error(
        "failed to instantiate WASM interface {interface_id:?}:{instance_id} from `{path}`: {source}"
    )]
    Interface {
        path: String,
        interface_id: [u8; 16],
        instance_id: String,
        #[source]
        source: WasmPluginHostError,
    },
}

pub(crate) struct LoadedWasmPlugin {
    plugin_id: String,
    pipeline_event_hooks: BTreeMap<String, Arc<dyn PipelineEventHook>>,
    benchmark_sinks: BTreeMap<String, Arc<dyn BenchmarkSink>>,
    interfaces: Vec<PluginInterfaceRecord>,
}

impl std::fmt::Debug for LoadedWasmPlugin {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoadedWasmPlugin")
            .field("plugin_id", &self.plugin_id)
            .field("interfaces", &self.interfaces)
            .finish_non_exhaustive()
    }
}

impl LoadedWasmPlugin {
    pub(crate) fn load(
        artifact: &WasmPluginArtifact,
        runtime: &WasmPluginRuntime,
    ) -> Result<Self, WasmPluginLoadError> {
        let bytes = read_component(artifact.path())?;
        let mut pipeline_event_hooks = BTreeMap::new();
        let mut benchmark_sinks = BTreeMap::new();
        let mut interfaces = Vec::with_capacity(artifact.interfaces.len());

        for declaration in &artifact.interfaces {
            if declaration.interface_id == PIPELINE_EVENT_HOOK_INTERFACE_ID.0 {
                let adapter =
                    WasmPipelineEventHookAdapter::from_component_bytes(runtime, &bytes)
                        .map_err(|source| interface_load_error(artifact, declaration, source))?;
                pipeline_event_hooks.insert(
                    declaration.instance_id.clone(),
                    Arc::new(adapter) as Arc<dyn PipelineEventHook>,
                );
            } else if declaration.interface_id == BENCHMARK_SINK_INTERFACE_ID.0 {
                let adapter = WasmBenchmarkSinkAdapter::from_component_bytes(
                    declaration.instance_id.clone(),
                    runtime,
                    &bytes,
                )
                .map_err(|source| interface_load_error(artifact, declaration, source))?;
                benchmark_sinks.insert(
                    declaration.instance_id.clone(),
                    Arc::new(adapter) as Arc<dyn BenchmarkSink>,
                );
            }
            interfaces.push(PluginInterfaceRecord {
                metadata: PluginInterfaceMetadata {
                    interface_id: declaration.interface_id,
                    major: declaration.major,
                    minor: declaration.minor,
                    instance_id: declaration.instance_id.clone(),
                },
                state: PluginInterfaceState::Available,
            });
        }

        Ok(Self {
            plugin_id: artifact.plugin_id.clone(),
            pipeline_event_hooks,
            benchmark_sinks,
            interfaces,
        })
    }

    pub(crate) fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub(crate) fn interfaces(&self) -> &[PluginInterfaceRecord] {
        &self.interfaces
    }

    pub(crate) fn resolve_pipeline_event_hook(
        &self,
        reference: &PluginReference,
    ) -> Result<(String, Arc<dyn PipelineEventHook>), PluginSelectionError> {
        self.select(reference, "PipelineEventHook", &self.pipeline_event_hooks)
    }

    pub(crate) fn resolve_benchmark_sink(
        &self,
        reference: &PluginReference,
    ) -> Result<(String, Arc<dyn BenchmarkSink>), PluginSelectionError> {
        self.select(reference, "BenchmarkSink", &self.benchmark_sinks)
    }

    fn select<T: ?Sized>(
        &self,
        reference: &PluginReference,
        interface: &'static str,
        capabilities: &BTreeMap<String, Arc<T>>,
    ) -> Result<(String, Arc<T>), PluginSelectionError> {
        if reference.transport() != PluginTransport::Wasm {
            return Err(PluginSelectionError::TransportMismatch {
                expected: PluginTransport::Wasm,
                actual: reference.transport(),
            });
        }
        if reference.plugin_id() != self.plugin_id {
            return Err(PluginSelectionError::PluginIdMismatch {
                requested: reference.plugin_id().to_owned(),
                loaded: self.plugin_id.clone(),
            });
        }
        if let Some(instance_id) = reference.capability_instance_id() {
            return capabilities
                .get(instance_id)
                .cloned()
                .map(|capability| (instance_id.to_owned(), capability))
                .ok_or_else(|| PluginSelectionError::InstanceNotFound {
                    plugin_id: self.plugin_id.clone(),
                    interface,
                    instance_id: instance_id.to_owned(),
                });
        }

        match capabilities.len() {
            0 => Err(PluginSelectionError::InterfaceNotFound {
                plugin_id: self.plugin_id.clone(),
                interface,
            }),
            1 => capabilities
                .first_key_value()
                .map(|(instance_id, capability)| (instance_id.clone(), capability.clone()))
                .ok_or_else(|| PluginSelectionError::InterfaceNotFound {
                    plugin_id: self.plugin_id.clone(),
                    interface,
                }),
            count => Err(PluginSelectionError::Ambiguous {
                plugin_id: self.plugin_id.clone(),
                interface,
                count,
            }),
        }
    }
}

fn read_component(path: &Path) -> Result<Vec<u8>, WasmPluginLoadError> {
    let display_path = path.display().to_string();
    let file = File::open(path).map_err(|source| WasmPluginLoadError::Open {
        path: display_path.clone(),
        source,
    })?;
    let read_limit = u64::try_from(MAX_WASM_PLUGIN_COMPONENT_BYTES)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut reader = file.take(read_limit);
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|source| WasmPluginLoadError::Read {
            path: display_path.clone(),
            source,
        })?;
    if bytes.len() > MAX_WASM_PLUGIN_COMPONENT_BYTES {
        return Err(WasmPluginLoadError::ComponentTooLarge {
            path: display_path,
            limit: MAX_WASM_PLUGIN_COMPONENT_BYTES,
        });
    }
    Ok(bytes)
}

fn interface_load_error(
    artifact: &WasmPluginArtifact,
    declaration: &WasmPluginInterfaceDeclaration,
    source: WasmPluginHostError,
) -> WasmPluginLoadError {
    WasmPluginLoadError::Interface {
        path: artifact.path.display().to_string(),
        interface_id: declaration.interface_id,
        instance_id: declaration.instance_id.clone(),
        source,
    }
}

fn is_supported_interface(interface_id: [u8; 16]) -> bool {
    interface_id == PIPELINE_EVENT_HOOK_INTERFACE_ID.0
        || interface_id == BENCHMARK_SINK_INTERFACE_ID.0
}
