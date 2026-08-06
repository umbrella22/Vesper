use super::*;
use crate::diagnostics::{
    decoder_capability_summary, source_normalizer_packet_capability_summary,
    source_normalizer_resource_capability_summary,
};
use player_plugin::{PluginReference, PluginReferenceError, PluginTransport};
#[cfg(feature = "installed-catalog")]
use player_plugin_package::{
    PluginArtifactTransport, VerifiedInstalledArtifact, VerifiedInstalledPluginCatalog,
};
#[cfg(feature = "wasm")]
use player_plugin_wasm_host::{WasmPluginRuntime, WasmPluginRuntimeError};
#[cfg(feature = "installed-catalog")]
use std::collections::BTreeSet;
use std::collections::HashMap;

/// One verified native artifact entry supplied by a host-owned plugin catalog.
///
/// The path is an internal locator. Capability selection always uses a
/// [`PluginReference`], and loading verifies that the Root ABI identity matches
/// `plugin_id` exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativePluginArtifact {
    plugin_id: String,
    path: PathBuf,
}

impl NativePluginArtifact {
    pub fn new(
        plugin_id: impl Into<String>,
        path: impl Into<PathBuf>,
    ) -> Result<Self, PluginReferenceError> {
        let plugin_id = plugin_id.into();
        PluginReference::new(plugin_id.clone(), None, PluginTransport::Native)?;
        Ok(Self {
            plugin_id,
            path: path.into(),
        })
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PluginIdentityKey {
    transport: PluginTransport,
    plugin_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PluginInterfaceKey {
    transport: PluginTransport,
    plugin_id: String,
    interface_id: [u8; 16],
    instance_id: String,
}

fn interface_references(
    plugin: &LoadedNativePlugin,
    interface_id: [u8; 16],
) -> Result<Vec<PluginReference>, PluginReferenceError> {
    plugin
        .interfaces()
        .iter()
        .filter(|interface| interface.metadata.interface_id == interface_id)
        .map(|interface| {
            PluginReference::new(
                plugin.plugin_id(),
                Some(interface.metadata.instance_id.clone()),
                PluginTransport::Native,
            )
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredPluginInterface {
    pub artifact_path: PathBuf,
    pub transport: PluginTransport,
    pub plugin_id: String,
    pub interface: PluginInterfaceRecord,
}

#[derive(Clone)]
pub struct ResolvedPluginCapability<T: ?Sized> {
    reference: PluginReference,
    capability: Arc<T>,
}

impl<T: ?Sized> std::fmt::Debug for ResolvedPluginCapability<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResolvedPluginCapability")
            .field("reference", &self.reference)
            .finish_non_exhaustive()
    }
}

impl<T: ?Sized> ResolvedPluginCapability<T> {
    pub fn reference(&self) -> &PluginReference {
        &self.reference
    }

    pub fn capability(&self) -> Arc<T> {
        self.capability.clone()
    }
}

#[derive(Debug, Error)]
pub enum PluginRegistryBuildError {
    #[error("failed to load native plugin artifact `{path}`: {source}")]
    Load {
        path: String,
        #[source]
        source: PluginLoadError,
    },
    #[error(
        "native plugin artifact `{path}` declared identity `{expected_plugin_id}` but its Root ABI reports `{actual_plugin_id}`"
    )]
    PluginIdentityMismatch {
        path: String,
        expected_plugin_id: String,
        actual_plugin_id: String,
    },
    #[error(
        "duplicate plugin identity {transport:?}:{plugin_id} from `{first_path}` and `{duplicate_path}`"
    )]
    DuplicatePluginIdentity {
        transport: PluginTransport,
        plugin_id: String,
        first_path: String,
        duplicate_path: String,
    },
    #[error(
        "duplicate interface identity {transport:?}:{plugin_id}:{interface_id:?}:{instance_id}"
    )]
    DuplicateInterfaceIdentity {
        transport: PluginTransport,
        plugin_id: String,
        interface_id: [u8; 16],
        instance_id: String,
    },
    #[cfg(feature = "wasm")]
    #[error("failed to initialize the WASM plugin runtime: {source}")]
    WasmRuntime {
        #[source]
        source: WasmPluginRuntimeError,
    },
    #[cfg(feature = "wasm")]
    #[error("failed to load WASM plugin artifact `{path}` for `{plugin_id}`: {source}")]
    WasmLoad {
        path: String,
        plugin_id: String,
        #[source]
        source: WasmPluginLoadError,
    },
    #[cfg(feature = "installed-catalog")]
    #[error("invalid verified installed plugin artifact: {message}")]
    InstalledCatalog { message: String },
    #[cfg(feature = "installed-catalog")]
    #[error(
        "installed plugin artifact `{path}` for `{plugin_id}` does not match its declared capabilities: {message}"
    )]
    InstalledCapabilityMismatch {
        path: String,
        plugin_id: String,
        message: String,
    },
    #[cfg(all(feature = "installed-catalog", not(feature = "wasm")))]
    #[error("installed WASM plugin `{plugin_id}` requires the loader `wasm` feature")]
    InstalledWasmUnsupported { plugin_id: String },
}

/// Aggregated loader-side report for inspected dynamic plugin paths.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PluginRegistryReport {
    pub total: usize,
    pub loaded: usize,
    pub failed: usize,
    pub decoder_supported: usize,
    pub decoder_unsupported: usize,
    pub frame_processor_supported: usize,
    pub frame_processor_unsupported: usize,
    pub source_normalizer_supported: usize,
    pub source_normalizer_unsupported: usize,
    pub unsupported_kind: usize,
    pub best_supported_decoder_name: Option<String>,
    pub best_supported_frame_processor_name: Option<String>,
    pub best_supported_source_normalizer_name: Option<String>,
    pub diagnostic_notes: Vec<String>,
}

/// Structured report for dynamic plugins loaded from host-provided paths.
#[derive(Debug, Clone, Default)]
pub struct PluginRegistry {
    records: Vec<PluginDiagnosticRecord>,
    record_references: Vec<Option<PluginReference>>,
    plugins: HashMap<PluginIdentityKey, Arc<LoadedNativePlugin>>,
    #[cfg(feature = "wasm")]
    wasm_plugins: HashMap<PluginIdentityKey, Arc<LoadedWasmPlugin>>,
    plugin_paths: HashMap<PluginIdentityKey, PathBuf>,
    interfaces: Vec<RegisteredPluginInterface>,
    interface_index: HashMap<PluginInterfaceKey, usize>,
}

impl PluginRegistry {
    /// Inspects unsigned raw native libraries under explicit development policy.
    pub fn inspect_decoder_support_development(
        paths: impl IntoIterator<Item = impl AsRef<Path>>,
        request: DecoderPluginMatchRequest,
    ) -> Self {
        let mut registry = Self::default();
        for path in paths {
            let path = path.as_ref().to_path_buf();
            let Some(plugin) = registry.load_inspected_native_development(&path) else {
                continue;
            };
            let Some(references) = registry.inspected_interface_references(
                &path,
                &plugin,
                player_plugin_abi::NATIVE_DECODER_INTERFACE_ID.0,
            ) else {
                continue;
            };
            if references.is_empty() {
                registry.push_record(
                    PluginDiagnosticRecord::unsupported_native_interface(
                        path,
                        &plugin,
                        "NativeDecoder",
                    ),
                    None,
                );
                continue;
            }
            for reference in references {
                let record = PluginDiagnosticRecord::from_native_decoder_interface(
                    path.clone(),
                    &plugin,
                    &reference,
                    &request,
                );
                registry.push_record(record, Some(reference));
            }
        }
        registry
    }

    /// Inspects unsigned raw native libraries under explicit development policy.
    pub fn inspect_frame_processor_support_development(
        paths: impl IntoIterator<Item = impl AsRef<Path>>,
    ) -> Self {
        let mut registry = Self::default();
        for path in paths {
            let path = path.as_ref().to_path_buf();
            let Some(plugin) = registry.load_inspected_native_development(&path) else {
                continue;
            };
            let Some(references) = registry.inspected_interface_references(
                &path,
                &plugin,
                player_plugin_abi::FRAME_PROCESSOR_INTERFACE_ID.0,
            ) else {
                continue;
            };
            if references.is_empty() {
                registry.push_record(
                    PluginDiagnosticRecord::unsupported_native_interface(
                        path,
                        &plugin,
                        "FrameProcessor",
                    ),
                    None,
                );
                continue;
            }
            for reference in references {
                let record = PluginDiagnosticRecord::from_native_frame_processor_interface(
                    path.clone(),
                    &plugin,
                    &reference,
                );
                registry.push_record(record, Some(reference));
            }
        }
        registry
    }

    /// Inspects host-verified native artifacts and binds each declared plugin
    /// identity to the Root ABI identity before exposing capability records.
    pub fn inspect_frame_processor_support_artifacts(
        artifacts: impl IntoIterator<Item = NativePluginArtifact>,
    ) -> Self {
        let mut registry = Self::default();
        for artifact in artifacts {
            let path = artifact.path.clone();
            let Some(plugin) = registry.load_inspected_native_artifact(&artifact) else {
                continue;
            };
            let Some(references) = registry.inspected_interface_references(
                &path,
                &plugin,
                player_plugin_abi::FRAME_PROCESSOR_INTERFACE_ID.0,
            ) else {
                continue;
            };
            if references.is_empty() {
                registry.push_record(
                    PluginDiagnosticRecord::unsupported_native_interface(
                        path,
                        &plugin,
                        "FrameProcessor",
                    ),
                    None,
                );
                continue;
            }
            for reference in references {
                let record = PluginDiagnosticRecord::from_native_frame_processor_interface(
                    path.clone(),
                    &plugin,
                    &reference,
                );
                registry.push_record(record, Some(reference));
            }
        }
        registry
    }

    /// Inspects unsigned raw native libraries under explicit development policy.
    pub fn inspect_source_normalizer_support_development(
        paths: impl IntoIterator<Item = impl AsRef<Path>>,
    ) -> Self {
        let mut registry = Self::default();
        for path in paths {
            let path = path.as_ref().to_path_buf();
            let Some(plugin) = registry.load_inspected_native_development(&path) else {
                continue;
            };
            let Some(packet_references) = registry.inspected_interface_references(
                &path,
                &plugin,
                player_plugin_abi::SOURCE_NORMALIZER_PACKET_INTERFACE_ID.0,
            ) else {
                continue;
            };
            let Some(resource_references) = registry.inspected_interface_references(
                &path,
                &plugin,
                player_plugin_abi::SOURCE_NORMALIZER_RESOURCE_INTERFACE_ID.0,
            ) else {
                continue;
            };
            if packet_references.is_empty() && resource_references.is_empty() {
                registry.push_record(
                    PluginDiagnosticRecord::unsupported_native_interface(
                        path,
                        &plugin,
                        "SourceNormalizerPacket or SourceNormalizerResource",
                    ),
                    None,
                );
                continue;
            }
            for reference in resource_references {
                let record = PluginDiagnosticRecord::from_native_source_resource_interface(
                    path.clone(),
                    &plugin,
                    &reference,
                );
                registry.push_record(record, Some(reference));
            }
            for reference in packet_references {
                let record = PluginDiagnosticRecord::from_native_source_packet_interface(
                    path.clone(),
                    &plugin,
                    &reference,
                );
                registry.push_record(record, Some(reference));
            }
        }
        registry
    }

    /// Inspects host-verified native artifacts and binds each declared plugin
    /// identity to the Root ABI identity before exposing capability records.
    pub fn inspect_source_normalizer_support_artifacts(
        artifacts: impl IntoIterator<Item = NativePluginArtifact>,
    ) -> Self {
        let mut registry = Self::default();
        for artifact in artifacts {
            let path = artifact.path.clone();
            let Some(plugin) = registry.load_inspected_native_artifact(&artifact) else {
                continue;
            };
            let Some(packet_references) = registry.inspected_interface_references(
                &path,
                &plugin,
                player_plugin_abi::SOURCE_NORMALIZER_PACKET_INTERFACE_ID.0,
            ) else {
                continue;
            };
            let Some(resource_references) = registry.inspected_interface_references(
                &path,
                &plugin,
                player_plugin_abi::SOURCE_NORMALIZER_RESOURCE_INTERFACE_ID.0,
            ) else {
                continue;
            };
            if packet_references.is_empty() && resource_references.is_empty() {
                registry.push_record(
                    PluginDiagnosticRecord::unsupported_native_interface(
                        path,
                        &plugin,
                        "SourceNormalizerPacket or SourceNormalizerResource",
                    ),
                    None,
                );
                continue;
            }
            for reference in resource_references {
                let record = PluginDiagnosticRecord::from_native_source_resource_interface(
                    path.clone(),
                    &plugin,
                    &reference,
                );
                registry.push_record(record, Some(reference));
            }
            for reference in packet_references {
                let record = PluginDiagnosticRecord::from_native_source_packet_interface(
                    path.clone(),
                    &plugin,
                    &reference,
                );
                registry.push_record(record, Some(reference));
            }
        }
        registry
    }

    pub fn from_records(records: Vec<PluginDiagnosticRecord>) -> Self {
        Self {
            record_references: vec![None; records.len()],
            records,
            ..Self::default()
        }
    }

    /// Builds a diagnostic-only registry from records with their validated
    /// canonical references already attached.
    ///
    /// This constructor does not load plugin instances. It is intended for
    /// host adapters and synthetic fixtures that have already established the
    /// same identity mapping produced by native or WASM inspection.
    pub fn from_records_with_references(
        entries: impl IntoIterator<Item = (PluginDiagnosticRecord, Option<PluginReference>)>,
    ) -> Self {
        let (records, record_references): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
        Self {
            records,
            record_references,
            ..Self::default()
        }
    }

    fn load_inspected_native_development(
        &mut self,
        path: &Path,
    ) -> Option<Arc<LoadedNativePlugin>> {
        let plugin = match LoadedNativePlugin::load_development(path) {
            Ok(plugin) => Arc::new(plugin),
            Err(error) => {
                self.push_record(
                    PluginDiagnosticRecord::load_failed(path.to_path_buf(), error),
                    None,
                );
                return None;
            }
        };
        self.register_inspected_native(path, plugin)
    }

    fn load_inspected_native_artifact(
        &mut self,
        artifact: &NativePluginArtifact,
    ) -> Option<Arc<LoadedNativePlugin>> {
        let path = artifact.path();
        let plugin = match LoadedNativePlugin::load_host_verified(path) {
            Ok(plugin) => Arc::new(plugin),
            Err(error) => {
                self.push_record(
                    PluginDiagnosticRecord::load_failed(path.to_path_buf(), error),
                    None,
                );
                return None;
            }
        };
        if plugin.plugin_id() != artifact.plugin_id() {
            let error = PluginRegistryBuildError::PluginIdentityMismatch {
                path: path.display().to_string(),
                expected_plugin_id: artifact.plugin_id().to_owned(),
                actual_plugin_id: plugin.plugin_id().to_owned(),
            };
            self.push_record(
                PluginDiagnosticRecord::load_failed_message(path.to_path_buf(), error.to_string()),
                None,
            );
            return None;
        }
        self.register_inspected_native(path, plugin)
    }

    fn register_inspected_native(
        &mut self,
        path: &Path,
        plugin: Arc<LoadedNativePlugin>,
    ) -> Option<Arc<LoadedNativePlugin>> {
        let root_diagnostics = plugin
            .diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.interface.is_none())
            .map(|diagnostic| diagnostic.message.clone())
            .collect::<Vec<_>>();
        if let Err(error) = self.insert_native(path.to_path_buf(), plugin.clone()) {
            self.push_record(
                PluginDiagnosticRecord::load_failed_message(path.to_path_buf(), error.to_string()),
                None,
            );
            return None;
        }
        for message in root_diagnostics {
            self.push_record(
                PluginDiagnosticRecord::load_failed_message(path.to_path_buf(), message),
                None,
            );
        }
        Some(plugin)
    }

    fn inspected_interface_references(
        &mut self,
        path: &Path,
        plugin: &LoadedNativePlugin,
        interface_id: [u8; 16],
    ) -> Option<Vec<PluginReference>> {
        match interface_references(plugin, interface_id) {
            Ok(references) => Some(references),
            Err(error) => {
                self.push_record(
                    PluginDiagnosticRecord::load_failed_message(
                        path.to_path_buf(),
                        format!("validated Root ABI identity could not form a reference: {error}"),
                    ),
                    None,
                );
                None
            }
        }
    }

    fn push_record(&mut self, record: PluginDiagnosticRecord, reference: Option<PluginReference>) {
        self.records.push(record);
        self.record_references.push(reference);
    }

    /// Loads unsigned raw native libraries under explicit development policy.
    pub fn load_native_development(
        paths: impl IntoIterator<Item = impl AsRef<Path>>,
    ) -> Result<Self, PluginRegistryBuildError> {
        let mut registry = Self::default();
        for path in paths {
            let path = path.as_ref().to_path_buf();
            let plugin = LoadedNativePlugin::load_development(&path).map_err(|source| {
                PluginRegistryBuildError::Load {
                    path: path.display().to_string(),
                    source,
                }
            })?;
            registry.insert_native(path, Arc::new(plugin))?;
        }
        Ok(registry)
    }

    pub fn load_native_artifacts(
        artifacts: impl IntoIterator<Item = NativePluginArtifact>,
    ) -> Result<Self, PluginRegistryBuildError> {
        let mut registry = Self::default();
        registry.extend_native_artifacts(artifacts)?;
        Ok(registry)
    }

    fn extend_native_artifacts(
        &mut self,
        artifacts: impl IntoIterator<Item = NativePluginArtifact>,
    ) -> Result<(), PluginRegistryBuildError> {
        for artifact in artifacts {
            let path = artifact.path;
            let plugin = LoadedNativePlugin::load_host_verified(&path).map_err(|source| {
                PluginRegistryBuildError::Load {
                    path: path.display().to_string(),
                    source,
                }
            })?;
            if plugin.plugin_id() != artifact.plugin_id {
                return Err(PluginRegistryBuildError::PluginIdentityMismatch {
                    path: path.display().to_string(),
                    expected_plugin_id: artifact.plugin_id,
                    actual_plugin_id: plugin.plugin_id().to_owned(),
                });
            }
            self.insert_native(path, Arc::new(plugin))?;
        }
        Ok(())
    }

    #[cfg(feature = "wasm")]
    pub fn load_wasm_artifacts(
        artifacts: impl IntoIterator<Item = WasmPluginArtifact>,
    ) -> Result<Self, PluginRegistryBuildError> {
        let mut registry = Self::default();
        registry.extend_wasm_artifacts(artifacts)?;
        Ok(registry)
    }

    #[cfg(feature = "wasm")]
    pub fn load_artifacts(
        native_artifacts: impl IntoIterator<Item = NativePluginArtifact>,
        wasm_artifacts: impl IntoIterator<Item = WasmPluginArtifact>,
    ) -> Result<Self, PluginRegistryBuildError> {
        let mut registry = Self::default();
        registry.extend_native_artifacts(native_artifacts)?;
        registry.extend_wasm_artifacts(wasm_artifacts)?;
        Ok(registry)
    }

    #[cfg(feature = "installed-catalog")]
    pub fn load_verified_installed_catalog(
        catalog: &VerifiedInstalledPluginCatalog,
    ) -> Result<Self, PluginRegistryBuildError> {
        let mut registry = Self::default();
        #[cfg(feature = "wasm")]
        let mut wasm_runtime = None;
        for artifact in catalog.artifacts() {
            match artifact.transport() {
                PluginArtifactTransport::Native => {
                    let plugin = LoadedNativePlugin::load_host_verified(artifact.snapshot_path())
                        .map_err(|source| PluginRegistryBuildError::Load {
                        path: artifact.installed_path().display().to_string(),
                        source,
                    })?;
                    if plugin.plugin_id() != artifact.plugin_id() {
                        return Err(PluginRegistryBuildError::PluginIdentityMismatch {
                            path: artifact.installed_path().display().to_string(),
                            expected_plugin_id: artifact.plugin_id().to_owned(),
                            actual_plugin_id: plugin.plugin_id().to_owned(),
                        });
                    }
                    validate_installed_native_capabilities(artifact, &plugin)?;
                    registry
                        .insert_native(artifact.installed_path().to_path_buf(), Arc::new(plugin))?;
                }
                PluginArtifactTransport::Wasm => {
                    #[cfg(feature = "wasm")]
                    {
                        let declarations = installed_wasm_declarations(artifact)?;
                        let declared = WasmPluginArtifact::new(
                            artifact.plugin_id(),
                            artifact.snapshot_path(),
                            declarations,
                        )
                        .map_err(|error| {
                            PluginRegistryBuildError::InstalledCatalog {
                                message: error.to_string(),
                            }
                        })?;
                        let runtime = match wasm_runtime.as_ref() {
                            Some(runtime) => runtime,
                            None => {
                                wasm_runtime =
                                    Some(WasmPluginRuntime::new().map_err(|source| {
                                        PluginRegistryBuildError::WasmRuntime { source }
                                    })?);
                                wasm_runtime.as_ref().ok_or_else(|| {
                                    PluginRegistryBuildError::InstalledCatalog {
                                        message: "WASM runtime initialization was lost".to_owned(),
                                    }
                                })?
                            }
                        };
                        let plugin =
                            LoadedWasmPlugin::load(&declared, runtime).map_err(|source| {
                                PluginRegistryBuildError::WasmLoad {
                                    path: artifact.installed_path().display().to_string(),
                                    plugin_id: artifact.plugin_id().to_owned(),
                                    source,
                                }
                            })?;
                        registry.insert_wasm(
                            artifact.installed_path().to_path_buf(),
                            Arc::new(plugin),
                        )?;
                    }
                    #[cfg(not(feature = "wasm"))]
                    return Err(PluginRegistryBuildError::InstalledWasmUnsupported {
                        plugin_id: artifact.plugin_id().to_owned(),
                    });
                }
            }
        }
        Ok(registry)
    }

    #[cfg(feature = "wasm")]
    fn extend_wasm_artifacts(
        &mut self,
        artifacts: impl IntoIterator<Item = WasmPluginArtifact>,
    ) -> Result<(), PluginRegistryBuildError> {
        let mut artifacts = artifacts.into_iter().peekable();
        if artifacts.peek().is_none() {
            return Ok(());
        }
        let runtime = WasmPluginRuntime::new()
            .map_err(|source| PluginRegistryBuildError::WasmRuntime { source })?;
        for artifact in artifacts {
            let path = artifact.path().to_path_buf();
            let plugin_id = artifact.plugin_id().to_owned();
            let plugin = LoadedWasmPlugin::load(&artifact, &runtime).map_err(|source| {
                PluginRegistryBuildError::WasmLoad {
                    path: path.display().to_string(),
                    plugin_id,
                    source,
                }
            })?;
            self.insert_wasm(path, Arc::new(plugin))?;
        }
        Ok(())
    }

    fn insert_native(
        &mut self,
        artifact_path: PathBuf,
        plugin: Arc<LoadedNativePlugin>,
    ) -> Result<(), PluginRegistryBuildError> {
        let identity = PluginIdentityKey {
            transport: PluginTransport::Native,
            plugin_id: plugin.plugin_id().to_owned(),
        };
        if let Some(first_path) = self.plugin_paths.get(&identity) {
            return Err(PluginRegistryBuildError::DuplicatePluginIdentity {
                transport: identity.transport,
                plugin_id: identity.plugin_id,
                first_path: first_path.display().to_string(),
                duplicate_path: artifact_path.display().to_string(),
            });
        }

        let mut pending = Vec::with_capacity(plugin.interfaces().len());
        for interface in plugin.interfaces() {
            let key = PluginInterfaceKey {
                transport: PluginTransport::Native,
                plugin_id: plugin.plugin_id().to_owned(),
                interface_id: interface.metadata.interface_id,
                instance_id: interface.metadata.instance_id.clone(),
            };
            if self.interface_index.contains_key(&key)
                || pending
                    .iter()
                    .any(|(pending_key, _): &(PluginInterfaceKey, _)| pending_key == &key)
            {
                return Err(PluginRegistryBuildError::DuplicateInterfaceIdentity {
                    transport: key.transport,
                    plugin_id: key.plugin_id,
                    interface_id: key.interface_id,
                    instance_id: key.instance_id,
                });
            }
            pending.push((
                key,
                RegisteredPluginInterface {
                    artifact_path: artifact_path.clone(),
                    transport: PluginTransport::Native,
                    plugin_id: plugin.plugin_id().to_owned(),
                    interface: interface.clone(),
                },
            ));
        }

        self.plugin_paths.insert(identity.clone(), artifact_path);
        self.plugins.insert(identity, plugin);
        for (key, interface) in pending {
            let index = self.interfaces.len();
            self.interfaces.push(interface);
            self.interface_index.insert(key, index);
        }
        Ok(())
    }

    #[cfg(feature = "wasm")]
    fn insert_wasm(
        &mut self,
        artifact_path: PathBuf,
        plugin: Arc<LoadedWasmPlugin>,
    ) -> Result<(), PluginRegistryBuildError> {
        let identity = PluginIdentityKey {
            transport: PluginTransport::Wasm,
            plugin_id: plugin.plugin_id().to_owned(),
        };
        if let Some(first_path) = self.plugin_paths.get(&identity) {
            return Err(PluginRegistryBuildError::DuplicatePluginIdentity {
                transport: identity.transport,
                plugin_id: identity.plugin_id,
                first_path: first_path.display().to_string(),
                duplicate_path: artifact_path.display().to_string(),
            });
        }

        let mut pending = Vec::with_capacity(plugin.interfaces().len());
        for interface in plugin.interfaces() {
            let key = PluginInterfaceKey {
                transport: PluginTransport::Wasm,
                plugin_id: plugin.plugin_id().to_owned(),
                interface_id: interface.metadata.interface_id,
                instance_id: interface.metadata.instance_id.clone(),
            };
            if self.interface_index.contains_key(&key)
                || pending
                    .iter()
                    .any(|(pending_key, _): &(PluginInterfaceKey, _)| pending_key == &key)
            {
                return Err(PluginRegistryBuildError::DuplicateInterfaceIdentity {
                    transport: key.transport,
                    plugin_id: key.plugin_id,
                    interface_id: key.interface_id,
                    instance_id: key.instance_id,
                });
            }
            pending.push((
                key,
                RegisteredPluginInterface {
                    artifact_path: artifact_path.clone(),
                    transport: PluginTransport::Wasm,
                    plugin_id: plugin.plugin_id().to_owned(),
                    interface: interface.clone(),
                },
            ));
        }

        self.plugin_paths.insert(identity.clone(), artifact_path);
        self.wasm_plugins.insert(identity, plugin);
        for (key, interface) in pending {
            let index = self.interfaces.len();
            self.interfaces.push(interface);
            self.interface_index.insert(key, index);
        }
        Ok(())
    }

    pub fn registered_interfaces(&self) -> &[RegisteredPluginInterface] {
        &self.interfaces
    }

    pub fn post_download_references(&self) -> Result<Vec<PluginReference>, PluginSelectionError> {
        self.references_for_interface(player_plugin_abi::POST_DOWNLOAD_PROCESSOR_INTERFACE_ID.0)
    }

    pub fn pipeline_event_hook_references(
        &self,
    ) -> Result<Vec<PluginReference>, PluginSelectionError> {
        self.references_for_interface(player_plugin_abi::PIPELINE_EVENT_HOOK_INTERFACE_ID.0)
    }

    pub fn benchmark_sink_references(&self) -> Result<Vec<PluginReference>, PluginSelectionError> {
        self.references_for_interface(player_plugin_abi::BENCHMARK_SINK_INTERFACE_ID.0)
    }

    pub fn native_decoder_references(&self) -> Result<Vec<PluginReference>, PluginSelectionError> {
        self.references_for_interface(player_plugin_abi::NATIVE_DECODER_INTERFACE_ID.0)
    }

    pub fn frame_processor_references(&self) -> Result<Vec<PluginReference>, PluginSelectionError> {
        self.references_for_interface(player_plugin_abi::FRAME_PROCESSOR_INTERFACE_ID.0)
    }

    pub fn source_packet_references(&self) -> Result<Vec<PluginReference>, PluginSelectionError> {
        self.references_for_interface(player_plugin_abi::SOURCE_NORMALIZER_PACKET_INTERFACE_ID.0)
    }

    pub fn source_resource_references(&self) -> Result<Vec<PluginReference>, PluginSelectionError> {
        self.references_for_interface(player_plugin_abi::SOURCE_NORMALIZER_RESOURCE_INTERFACE_ID.0)
    }

    fn references_for_interface(
        &self,
        interface_id: [u8; 16],
    ) -> Result<Vec<PluginReference>, PluginSelectionError> {
        self.interfaces
            .iter()
            .filter(|interface| {
                interface.interface.state == PluginInterfaceState::Available
                    && interface.interface.metadata.interface_id == interface_id
            })
            .map(|interface| {
                PluginReference::new(
                    interface.plugin_id.clone(),
                    Some(interface.interface.metadata.instance_id.clone()),
                    interface.transport,
                )
                .map_err(|_| PluginSelectionError::InvalidLoadedIdentity {
                    plugin_id: interface.plugin_id.clone(),
                    instance_id: interface.interface.metadata.instance_id.clone(),
                })
            })
            .collect()
    }

    pub fn resolve_post_download(
        &self,
        reference: &PluginReference,
    ) -> Result<ResolvedPluginCapability<dyn PostDownloadProcessor>, PluginSelectionError> {
        let plugin = self.plugin_for(reference)?;
        let (instance_id, capability) = plugin.resolve_post_download_selected(reference)?;
        self.resolved(reference, instance_id, capability)
    }

    pub fn resolve_pipeline_event_hook(
        &self,
        reference: &PluginReference,
    ) -> Result<ResolvedPluginCapability<dyn PipelineEventHook>, PluginSelectionError> {
        if reference.transport() == PluginTransport::Wasm {
            #[cfg(feature = "wasm")]
            {
                let plugin = self.wasm_plugin_for(reference)?;
                let (instance_id, capability) = plugin.resolve_pipeline_event_hook(reference)?;
                return self.resolved(reference, instance_id, capability);
            }
            #[cfg(not(feature = "wasm"))]
            {
                return Err(PluginSelectionError::PluginNotFound {
                    plugin_id: reference.plugin_id().to_owned(),
                    transport: PluginTransport::Wasm,
                });
            }
        }
        let plugin = self.plugin_for(reference)?;
        let (instance_id, capability) = plugin.resolve_pipeline_event_hook_selected(reference)?;
        self.resolved(reference, instance_id, capability)
    }

    pub fn resolve_benchmark_sink(
        &self,
        reference: &PluginReference,
    ) -> Result<ResolvedPluginCapability<dyn BenchmarkSink>, PluginSelectionError> {
        if reference.transport() == PluginTransport::Wasm {
            #[cfg(feature = "wasm")]
            {
                let plugin = self.wasm_plugin_for(reference)?;
                let (instance_id, capability) = plugin.resolve_benchmark_sink(reference)?;
                return self.resolved(reference, instance_id, capability);
            }
            #[cfg(not(feature = "wasm"))]
            {
                return Err(PluginSelectionError::PluginNotFound {
                    plugin_id: reference.plugin_id().to_owned(),
                    transport: PluginTransport::Wasm,
                });
            }
        }
        let plugin = self.plugin_for(reference)?;
        let (instance_id, capability) = plugin.resolve_benchmark_sink_selected(reference)?;
        self.resolved(reference, instance_id, capability)
    }

    pub fn resolve_native_decoder(
        &self,
        reference: &PluginReference,
    ) -> Result<ResolvedPluginCapability<dyn NativeDecoderPluginFactory>, PluginSelectionError>
    {
        let plugin = self.plugin_for(reference)?;
        let (instance_id, capability) = plugin.resolve_native_decoder_selected(reference)?;
        self.resolved(reference, instance_id, capability)
    }

    pub fn resolve_frame_processor(
        &self,
        reference: &PluginReference,
    ) -> Result<ResolvedPluginCapability<dyn FrameProcessorPluginFactory>, PluginSelectionError>
    {
        let plugin = self.plugin_for(reference)?;
        let (instance_id, capability) = plugin.resolve_frame_processor_selected(reference)?;
        self.resolved(reference, instance_id, capability)
    }

    pub fn resolve_source_packet(
        &self,
        reference: &PluginReference,
    ) -> Result<
        ResolvedPluginCapability<dyn SourceNormalizerPacketPluginFactory>,
        PluginSelectionError,
    > {
        let plugin = self.plugin_for(reference)?;
        let (instance_id, capability) = plugin.resolve_source_packet_selected(reference)?;
        self.resolved(reference, instance_id, capability)
    }

    pub fn resolve_source_resource(
        &self,
        reference: &PluginReference,
    ) -> Result<
        ResolvedPluginCapability<dyn SourceNormalizerResourcePluginFactory>,
        PluginSelectionError,
    > {
        let plugin = self.plugin_for(reference)?;
        let (instance_id, capability) = plugin.resolve_source_resource_selected(reference)?;
        self.resolved(reference, instance_id, capability)
    }

    fn plugin_for(
        &self,
        reference: &PluginReference,
    ) -> Result<Arc<LoadedNativePlugin>, PluginSelectionError> {
        let identity = PluginIdentityKey {
            transport: reference.transport(),
            plugin_id: reference.plugin_id().to_owned(),
        };
        self.plugins
            .get(&identity)
            .cloned()
            .ok_or_else(|| PluginSelectionError::PluginNotFound {
                plugin_id: reference.plugin_id().to_owned(),
                transport: reference.transport(),
            })
    }

    #[cfg(feature = "wasm")]
    fn wasm_plugin_for(
        &self,
        reference: &PluginReference,
    ) -> Result<Arc<LoadedWasmPlugin>, PluginSelectionError> {
        let identity = PluginIdentityKey {
            transport: reference.transport(),
            plugin_id: reference.plugin_id().to_owned(),
        };
        self.wasm_plugins.get(&identity).cloned().ok_or_else(|| {
            PluginSelectionError::PluginNotFound {
                plugin_id: reference.plugin_id().to_owned(),
                transport: reference.transport(),
            }
        })
    }

    fn resolved<T: ?Sized>(
        &self,
        reference: &PluginReference,
        instance_id: String,
        capability: Arc<T>,
    ) -> Result<ResolvedPluginCapability<T>, PluginSelectionError> {
        let canonical = PluginReference::new(
            reference.plugin_id(),
            Some(instance_id.clone()),
            reference.transport(),
        )
        .map_err(|_| PluginSelectionError::InvalidLoadedIdentity {
            plugin_id: reference.plugin_id().to_owned(),
            instance_id,
        })?;
        Ok(ResolvedPluginCapability {
            reference: canonical,
            capability,
        })
    }

    pub fn records(&self) -> &[PluginDiagnosticRecord] {
        &self.records
    }

    /// Returns the canonical plugin reference associated with an inspection record.
    pub fn reference_for_record(
        &self,
        record: &PluginDiagnosticRecord,
    ) -> Option<&PluginReference> {
        self.records
            .iter()
            .position(|candidate| std::ptr::eq(candidate, record))
            .and_then(|index| self.record_references.get(index))
            .and_then(Option::as_ref)
    }

    pub fn best_decoder_for(
        &self,
        request: &DecoderPluginMatchRequest,
    ) -> Option<&PluginDiagnosticRecord> {
        self.records.iter().find(|record| {
            record.status == PluginDiagnosticStatus::DecoderSupported
                && decoder_capability_summary(record).is_some_and(|capabilities| {
                    capabilities.typed_codecs.iter().any(|codec| {
                        codec.media_kind == request.media_kind
                            && codec.codec.eq_ignore_ascii_case(&request.codec)
                    })
                })
        })
    }

    pub fn best_native_decoder_for(
        &self,
        request: &DecoderPluginMatchRequest,
    ) -> Option<&PluginDiagnosticRecord> {
        self.records.iter().find(|record| {
            record.status == PluginDiagnosticStatus::DecoderSupported
                && decoder_capability_summary(record).is_some_and(|capabilities| {
                    capabilities.supports_native_frame_output
                        && capabilities.typed_codecs.iter().any(|codec| {
                            codec.media_kind == request.media_kind
                                && codec.codec.eq_ignore_ascii_case(&request.codec)
                        })
                })
        })
    }

    pub fn best_pcm_audio_decoder_for(
        &self,
        request: &DecoderPluginMatchRequest,
    ) -> Option<&PluginDiagnosticRecord> {
        if request.media_kind != DecoderMediaKind::Audio {
            return None;
        }
        self.records.iter().find(|record| {
            record.status == PluginDiagnosticStatus::DecoderSupported
                && decoder_capability_summary(record).is_some_and(|capabilities| {
                    capabilities.supports_pcm_frames
                        && capabilities.typed_codecs.iter().any(|codec| {
                            codec.media_kind == request.media_kind
                                && codec.codec.eq_ignore_ascii_case(&request.codec)
                        })
                })
        })
    }

    pub fn supports_decoder(&self, request: &DecoderPluginMatchRequest) -> bool {
        self.best_decoder_for(request).is_some()
    }

    pub fn supports_native_decoder(&self, request: &DecoderPluginMatchRequest) -> bool {
        self.best_native_decoder_for(request).is_some()
    }

    pub fn supports_pcm_audio_decoder(&self, request: &DecoderPluginMatchRequest) -> bool {
        self.best_pcm_audio_decoder_for(request).is_some()
    }

    pub fn frame_processor_supported_plugin_names(&self) -> Vec<&str> {
        self.records
            .iter()
            .filter(|record| record.status == PluginDiagnosticStatus::FrameProcessorSupported)
            .filter_map(|record| record.plugin_name.as_deref())
            .collect()
    }

    pub fn source_normalizer_supported_plugin_names(&self) -> Vec<&str> {
        self.records
            .iter()
            .filter(|record| record.status == PluginDiagnosticStatus::SourceNormalizerSupported)
            .filter_map(|record| record.plugin_name.as_deref())
            .collect()
    }

    pub fn best_source_normalizer(&self) -> Option<&PluginDiagnosticRecord> {
        self.records
            .iter()
            .find(|record| record.status == PluginDiagnosticStatus::SourceNormalizerSupported)
    }

    pub fn best_source_normalizer_packet(&self) -> Option<&PluginDiagnosticRecord> {
        self.records.iter().find(|record| {
            record.status == PluginDiagnosticStatus::SourceNormalizerSupported
                && source_normalizer_packet_capability_summary(record).is_some()
        })
    }

    pub fn best_source_normalizer_packet_for_profile(
        &self,
        runtime_profile: &str,
    ) -> Option<&PluginDiagnosticRecord> {
        self.records.iter().find(|record| {
            record.status == PluginDiagnosticStatus::SourceNormalizerSupported
                && source_normalizer_packet_capability_summary(record).is_some_and(|capabilities| {
                    capabilities
                        .supported_runtime_profiles
                        .iter()
                        .any(|profile| profile.eq_ignore_ascii_case(runtime_profile))
                })
        })
    }

    pub fn best_source_normalizer_resource(&self) -> Option<&PluginDiagnosticRecord> {
        self.records.iter().find(|record| {
            record.status == PluginDiagnosticStatus::SourceNormalizerSupported
                && source_normalizer_resource_capability_summary(record).is_some()
        })
    }

    pub fn best_source_normalizer_resource_for_profile(
        &self,
        runtime_profile: &str,
    ) -> Option<&PluginDiagnosticRecord> {
        self.records.iter().find(|record| {
            record.status == PluginDiagnosticStatus::SourceNormalizerSupported
                && source_normalizer_resource_capability_summary(record).is_some_and(
                    |capabilities| {
                        capabilities
                            .supported_runtime_profiles
                            .iter()
                            .any(|profile| profile.eq_ignore_ascii_case(runtime_profile))
                    },
                )
        })
    }

    pub fn best_source_normalizer_for_profile(
        &self,
        runtime_profile: &str,
    ) -> Option<&PluginDiagnosticRecord> {
        self.records.iter().find(|record| {
            record.status == PluginDiagnosticStatus::SourceNormalizerSupported
                && (source_normalizer_resource_capability_summary(record).is_some_and(
                    |capabilities| {
                        capabilities
                            .supported_runtime_profiles
                            .iter()
                            .any(|profile| profile.eq_ignore_ascii_case(runtime_profile))
                    },
                ) || source_normalizer_packet_capability_summary(record).is_some_and(
                    |capabilities| {
                        capabilities
                            .supported_runtime_profiles
                            .iter()
                            .any(|profile| profile.eq_ignore_ascii_case(runtime_profile))
                    },
                ))
        })
    }

    pub fn decoder_supported_plugin_names(&self) -> Vec<&str> {
        self.records
            .iter()
            .filter(|record| record.status == PluginDiagnosticStatus::DecoderSupported)
            .filter_map(|record| record.plugin_name.as_deref())
            .collect()
    }

    pub fn diagnostic_notes(&self) -> Vec<String> {
        self.records
            .iter()
            .filter(|record| {
                !matches!(
                    record.status,
                    PluginDiagnosticStatus::DecoderSupported
                        | PluginDiagnosticStatus::FrameProcessorSupported
                        | PluginDiagnosticStatus::SourceNormalizerSupported
                )
            })
            .map(PluginDiagnosticRecord::summary)
            .collect()
    }

    pub fn report(&self) -> PluginRegistryReport {
        let mut report = PluginRegistryReport {
            total: self.records.len(),
            ..PluginRegistryReport::default()
        };

        for record in &self.records {
            match record.status {
                PluginDiagnosticStatus::Loaded => {
                    report.loaded += 1;
                    report.diagnostic_notes.push(record.summary());
                }
                PluginDiagnosticStatus::LoadFailed => {
                    report.failed += 1;
                    report.diagnostic_notes.push(record.summary());
                }
                PluginDiagnosticStatus::UnsupportedKind => {
                    report.loaded += 1;
                    report.unsupported_kind += 1;
                    report.diagnostic_notes.push(record.summary());
                }
                PluginDiagnosticStatus::DecoderSupported => {
                    report.loaded += 1;
                    report.decoder_supported += 1;
                    if report.best_supported_decoder_name.is_none() {
                        report.best_supported_decoder_name = record.plugin_name.clone();
                    }
                }
                PluginDiagnosticStatus::DecoderUnsupported => {
                    report.loaded += 1;
                    report.decoder_unsupported += 1;
                    report.diagnostic_notes.push(record.summary());
                }
                PluginDiagnosticStatus::FrameProcessorSupported => {
                    report.loaded += 1;
                    report.frame_processor_supported += 1;
                    if report.best_supported_frame_processor_name.is_none() {
                        report.best_supported_frame_processor_name = record.plugin_name.clone();
                    }
                }
                PluginDiagnosticStatus::FrameProcessorUnsupported => {
                    report.loaded += 1;
                    report.frame_processor_unsupported += 1;
                    report.diagnostic_notes.push(record.summary());
                }
                PluginDiagnosticStatus::SourceNormalizerSupported => {
                    report.loaded += 1;
                    report.source_normalizer_supported += 1;
                    if report.best_supported_source_normalizer_name.is_none() {
                        report.best_supported_source_normalizer_name = record.plugin_name.clone();
                    }
                }
                PluginDiagnosticStatus::SourceNormalizerUnsupported => {
                    report.loaded += 1;
                    report.source_normalizer_unsupported += 1;
                    report.diagnostic_notes.push(record.summary());
                }
            }
        }

        report
    }
}

#[cfg(feature = "installed-catalog")]
fn validate_installed_native_capabilities(
    artifact: &VerifiedInstalledArtifact,
    plugin: &LoadedNativePlugin,
) -> Result<(), PluginRegistryBuildError> {
    let declared = artifact
        .capabilities()
        .iter()
        .map(|capability| {
            let interface_id = uuid::Uuid::parse_str(&capability.interface_id)
                .map(|interface_id| *interface_id.as_bytes())
                .map_err(|error| PluginRegistryBuildError::InstalledCatalog {
                    message: format!(
                        "invalid interface UUID '{}': {error}",
                        capability.interface_id
                    ),
                })?;
            Ok((
                interface_id,
                capability.instance_id.clone(),
                capability.interface_major,
                capability.interface_minor,
            ))
        })
        .collect::<Result<BTreeSet<_>, PluginRegistryBuildError>>()?;
    let actual = plugin
        .interfaces()
        .iter()
        .filter(|interface| interface.state == PluginInterfaceState::Available)
        .map(|interface| {
            (
                interface.metadata.interface_id,
                interface.metadata.instance_id.clone(),
                interface.metadata.major,
                interface.metadata.minor,
            )
        })
        .collect::<BTreeSet<_>>();
    if declared != actual {
        return Err(PluginRegistryBuildError::InstalledCapabilityMismatch {
            path: artifact.installed_path().display().to_string(),
            plugin_id: artifact.plugin_id().to_owned(),
            message: format!("declared {declared:?}, Root ABI reported {actual:?}"),
        });
    }
    Ok(())
}

#[cfg(all(feature = "installed-catalog", feature = "wasm"))]
fn installed_wasm_declarations(
    artifact: &VerifiedInstalledArtifact,
) -> Result<Vec<WasmPluginInterfaceDeclaration>, PluginRegistryBuildError> {
    artifact
        .capabilities()
        .iter()
        .map(|capability| {
            let interface_id = uuid::Uuid::parse_str(&capability.interface_id)
                .map(|interface_id| *interface_id.as_bytes())
                .map_err(|error| PluginRegistryBuildError::InstalledCatalog {
                    message: format!(
                        "invalid interface UUID '{}': {error}",
                        capability.interface_id
                    ),
                })?;
            Ok(WasmPluginInterfaceDeclaration::new(
                interface_id,
                capability.interface_major,
                capability.interface_minor,
                capability.instance_id.clone(),
            ))
        })
        .collect()
}
