use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use thiserror::Error;

use crate::capability_ids::*;

use player_plugin::{
    AudioProcessorPluginFactory, FrameProcessorPluginFactory, NativeDecoderPluginFactory,
    PluginInvocationPolicyError, PluginReference, PluginTransport, PostDownloadProcessor,
    SourceNormalizerPacketPluginFactory, SourceNormalizerResourcePluginFactory,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginContractDiagnosticKind {
    Compatibility,
    ContractViolation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInterfaceMetadata {
    pub interface_id: [u8; 16],
    pub major: u16,
    pub minor: u16,
    pub instance_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginInterfaceState {
    Available,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInterfaceRecord {
    pub metadata: PluginInterfaceMetadata,
    pub state: PluginInterfaceState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInterfaceDiagnostic {
    pub index: Option<u32>,
    pub interface: Option<PluginInterfaceMetadata>,
    pub kind: PluginContractDiagnosticKind,
    pub message: String,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PluginSelectionError {
    #[error(transparent)]
    InvocationPolicyRejected(#[from] PluginInvocationPolicyError),
    #[error("plugin reference selects transport {actual:?}, but this root uses {expected:?}")]
    TransportMismatch {
        expected: PluginTransport,
        actual: PluginTransport,
    },
    #[error("plugin reference id `{requested}` does not match loaded plugin `{loaded}`")]
    PluginIdMismatch { requested: String, loaded: String },
    #[error("plugin `{plugin_id}` does not expose {interface} instance `{instance_id}`")]
    InstanceNotFound {
        plugin_id: String,
        interface: &'static str,
        instance_id: String,
    },
    #[error("plugin `{plugin_id}` does not expose interface {interface}")]
    InterfaceNotFound {
        plugin_id: String,
        interface: &'static str,
    },
    #[error(
        "plugin `{plugin_id}` exposes {interface} instance `{instance_id}`, but it is unavailable"
    )]
    InstanceUnavailable {
        plugin_id: String,
        interface: &'static str,
        instance_id: String,
    },
    #[error("plugin `{plugin_id}` exposes interface {interface}, but it is unavailable")]
    InterfaceUnavailable {
        plugin_id: String,
        interface: &'static str,
    },
    #[error(
        "plugin `{plugin_id}` exposes {count} instances of {interface}; capability_instance_id is required"
    )]
    Ambiguous {
        plugin_id: String,
        interface: &'static str,
        count: usize,
    },
    #[error("plugin `{plugin_id}` is not loaded for transport {transport:?}")]
    PluginNotFound {
        plugin_id: String,
        transport: PluginTransport,
    },
    #[error(
        "loaded plugin identity `{plugin_id}` or instance `{instance_id}` cannot form a canonical reference"
    )]
    InvalidLoadedIdentity {
        plugin_id: String,
        instance_id: String,
    },
}

pub struct LoadedNativePlugin {
    plugin_id: String,
    plugin_name: String,
    post_download: BTreeMap<String, Arc<dyn PostDownloadProcessor>>,
    native_decoders: BTreeMap<String, Arc<dyn NativeDecoderPluginFactory>>,
    source_packets: BTreeMap<String, Arc<dyn SourceNormalizerPacketPluginFactory>>,
    source_resources: BTreeMap<String, Arc<dyn SourceNormalizerResourcePluginFactory>>,
    advertised_instances: BTreeMap<[u8; 16], BTreeSet<String>>,
    interfaces: Vec<PluginInterfaceRecord>,
    diagnostics: Vec<PluginInterfaceDiagnostic>,
    native_plugin: crate::native::NativePlugin,
}

impl std::fmt::Debug for LoadedNativePlugin {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoadedNativePlugin")
            .field("plugin_id", &self.plugin_id)
            .field("plugin_name", &self.plugin_name)
            .field("post_download_instances", &self.post_download.keys())
            .field("native_decoder_instances", &self.native_decoders.keys())
            .field("source_packet_instances", &self.source_packets.keys())
            .field("source_resource_instances", &self.source_resources.keys())
            .field("advertised_instances", &self.advertised_instances)
            .field("interfaces", &self.interfaces)
            .field("diagnostics", &self.diagnostics)
            .finish()
    }
}

impl LoadedNativePlugin {
    pub(crate) fn native_plugin(&self) -> &crate::native::NativePlugin {
        &self.native_plugin
    }

    pub(crate) fn from_native_plugin(
        plugin: crate::native::NativePlugin,
    ) -> player_plugin::author::PluginResult<Self> {
        use player_plugin_abi as abi;

        let mut source_packets = BTreeMap::new();
        let mut source_resources = BTreeMap::new();
        let mut post_download = BTreeMap::new();
        let mut native_decoders = BTreeMap::new();
        let mut advertised_instances = BTreeMap::<[u8; 16], BTreeSet<String>>::new();
        let mut interfaces = Vec::new();

        // Keep host capability references stable across the binary ABI change.
        for &id in plugin.interface_ids() {
            if !plugin.inspect_interface(id)? {
                interfaces.push(PluginInterfaceRecord {
                    metadata: PluginInterfaceMetadata {
                        interface_id: id.bytes,
                        major: 0,
                        minor: 0,
                        instance_id: format!("{}.unknown-{}", plugin.id(), hex::encode(id.bytes)),
                    },
                    state: PluginInterfaceState::Unknown,
                });
                continue;
            }
            let (capability_id, instance_id) = match id {
                abi::VESPER_SOURCE_NORMALIZER_ID => {
                    let instance_id = format!("{}.packet", plugin.id());
                    source_packets.insert(instance_id.clone(), plugin.packet_factory()?);
                    (SOURCE_NORMALIZER_PACKET_INTERFACE_ID, instance_id)
                }
                abi::VESPER_SOURCE_NORMALIZER_RESOURCE_ID => {
                    let instance_id = format!("{}.resource", plugin.id());
                    source_resources.insert(instance_id.clone(), plugin.resource_factory()?);
                    (SOURCE_NORMALIZER_RESOURCE_INTERFACE_ID, instance_id)
                }
                abi::VESPER_POST_DOWNLOAD_PROCESSOR_ID => {
                    let instance_id = format!("{}.post-download", plugin.id());
                    post_download.insert(instance_id.clone(), plugin.post_download_processor()?);
                    (POST_DOWNLOAD_PROCESSOR_INTERFACE_ID, instance_id)
                }
                abi::VESPER_VIDEO_DECODER_ID => {
                    let instance_id = format!("{}.native", plugin.id());
                    native_decoders.insert(instance_id.clone(), plugin.native_decoder_factory()?);
                    (NATIVE_DECODER_INTERFACE_ID, instance_id)
                }
                abi::VESPER_AUDIO_PROCESSOR_ID => (id.bytes, format!("{}.audio", plugin.id())),
                abi::VESPER_VIDEO_FILTER_ID => (id.bytes, format!("{}.filter", plugin.id())),
                _ => {
                    return Err(player_plugin::author::PluginError::Contract(
                        "native plugin ABI interface requires a direct host registration",
                    ));
                }
            };
            advertised_instances
                .entry(capability_id)
                .or_default()
                .insert(instance_id.clone());
            interfaces.push(PluginInterfaceRecord {
                metadata: PluginInterfaceMetadata {
                    interface_id: capability_id,
                    major: 1,
                    minor: 0,
                    instance_id,
                },
                state: PluginInterfaceState::Available,
            });
        }

        Ok(Self {
            plugin_id: plugin.id().to_owned(),
            plugin_name: plugin.name().to_owned(),
            post_download,
            native_decoders,
            source_packets,
            source_resources,
            advertised_instances,
            interfaces,
            diagnostics: Vec::new(),
            native_plugin: plugin,
        })
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn plugin_name(&self) -> &str {
        &self.plugin_name
    }

    pub fn diagnostics(&self) -> &[PluginInterfaceDiagnostic] {
        &self.diagnostics
    }

    pub fn interfaces(&self) -> &[PluginInterfaceRecord] {
        &self.interfaces
    }

    pub fn unknown_interfaces(&self) -> impl Iterator<Item = &PluginInterfaceMetadata> {
        self.interfaces.iter().filter_map(|interface| {
            (interface.state == PluginInterfaceState::Unknown).then_some(&interface.metadata)
        })
    }

    pub fn resolve_post_download(
        &self,
        reference: &PluginReference,
    ) -> Result<Arc<dyn PostDownloadProcessor>, PluginSelectionError> {
        self.resolve_post_download_selected(reference)
            .map(|(_, capability)| capability)
    }

    pub(crate) fn resolve_post_download_selected(
        &self,
        reference: &PluginReference,
    ) -> Result<(String, Arc<dyn PostDownloadProcessor>), PluginSelectionError> {
        self.resolve(
            reference,
            POST_DOWNLOAD_PROCESSOR_INTERFACE_ID,
            "PostDownloadProcessor",
            Some(&self.post_download),
        )
    }

    pub fn resolve_native_decoder(
        &self,
        reference: &PluginReference,
    ) -> Result<Arc<dyn NativeDecoderPluginFactory>, PluginSelectionError> {
        self.resolve_native_decoder_selected(reference)
            .map(|(_, capability)| capability)
    }

    pub(crate) fn resolve_native_decoder_selected(
        &self,
        reference: &PluginReference,
    ) -> Result<(String, Arc<dyn NativeDecoderPluginFactory>), PluginSelectionError> {
        self.resolve(
            reference,
            NATIVE_DECODER_INTERFACE_ID,
            "NativeDecoder",
            Some(&self.native_decoders),
        )
    }

    pub fn resolve_frame_processor(
        &self,
        reference: &PluginReference,
    ) -> Result<Arc<dyn FrameProcessorPluginFactory>, PluginSelectionError> {
        self.resolve_frame_processor_selected(reference)
            .map(|(_, capability)| capability)
    }

    pub(crate) fn resolve_frame_processor_selected(
        &self,
        reference: &PluginReference,
    ) -> Result<(String, Arc<dyn FrameProcessorPluginFactory>), PluginSelectionError> {
        self.resolve(
            reference,
            FRAME_PROCESSOR_INTERFACE_ID,
            "FrameProcessor",
            None,
        )
    }

    pub fn resolve_audio_processor(
        &self,
        reference: &PluginReference,
    ) -> Result<Arc<dyn AudioProcessorPluginFactory>, PluginSelectionError> {
        self.resolve_audio_processor_selected(reference)
            .map(|(_, capability)| capability)
    }

    pub(crate) fn resolve_audio_processor_selected(
        &self,
        reference: &PluginReference,
    ) -> Result<(String, Arc<dyn AudioProcessorPluginFactory>), PluginSelectionError> {
        self.resolve(
            reference,
            AUDIO_PROCESSOR_INTERFACE_ID,
            "AudioProcessor",
            None,
        )
    }

    pub fn resolve_source_packet(
        &self,
        reference: &PluginReference,
    ) -> Result<Arc<dyn SourceNormalizerPacketPluginFactory>, PluginSelectionError> {
        self.resolve_source_packet_selected(reference)
            .map(|(_, capability)| capability)
    }

    pub(crate) fn resolve_source_packet_selected(
        &self,
        reference: &PluginReference,
    ) -> Result<(String, Arc<dyn SourceNormalizerPacketPluginFactory>), PluginSelectionError> {
        self.resolve(
            reference,
            SOURCE_NORMALIZER_PACKET_INTERFACE_ID,
            "SourceNormalizerPacket",
            Some(&self.source_packets),
        )
    }

    pub fn resolve_source_resource(
        &self,
        reference: &PluginReference,
    ) -> Result<Arc<dyn SourceNormalizerResourcePluginFactory>, PluginSelectionError> {
        self.resolve_source_resource_selected(reference)
            .map(|(_, capability)| capability)
    }

    pub(crate) fn resolve_source_resource_selected(
        &self,
        reference: &PluginReference,
    ) -> Result<(String, Arc<dyn SourceNormalizerResourcePluginFactory>), PluginSelectionError>
    {
        self.resolve(
            reference,
            SOURCE_NORMALIZER_RESOURCE_INTERFACE_ID,
            "SourceNormalizerResource",
            Some(&self.source_resources),
        )
    }

    fn resolve<T: ?Sized>(
        &self,
        reference: &PluginReference,
        interface_id: [u8; 16],
        interface: &'static str,
        instances: Option<&BTreeMap<String, Arc<T>>>,
    ) -> Result<(String, Arc<T>), PluginSelectionError> {
        if reference.transport() != PluginTransport::Native {
            return Err(PluginSelectionError::TransportMismatch {
                expected: PluginTransport::Native,
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
            return instances
                .and_then(|instances| instances.get(instance_id))
                .cloned()
                .map(|capability| (instance_id.to_owned(), capability))
                .ok_or_else(|| {
                    let advertised = self
                        .advertised_instances
                        .get(&interface_id)
                        .is_some_and(|values| values.contains(instance_id));
                    if advertised {
                        PluginSelectionError::InstanceUnavailable {
                            plugin_id: self.plugin_id.clone(),
                            interface,
                            instance_id: instance_id.to_owned(),
                        }
                    } else {
                        PluginSelectionError::InstanceNotFound {
                            plugin_id: self.plugin_id.clone(),
                            interface,
                            instance_id: instance_id.to_owned(),
                        }
                    }
                });
        }
        let advertised_count = self
            .advertised_instances
            .get(&interface_id)
            .map_or(0, BTreeSet::len);
        match advertised_count {
            0 => Err(PluginSelectionError::InterfaceNotFound {
                plugin_id: self.plugin_id.clone(),
                interface,
            }),
            1 => instances
                .and_then(|instances| instances.first_key_value())
                .map(|(instance_id, capability)| (instance_id.clone(), capability.clone()))
                .ok_or_else(|| PluginSelectionError::InterfaceUnavailable {
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

#[cfg(test)]
mod tests {
    use super::*;
    use player_plugin_abi::author::*;
    use player_plugin_abi::*;
    use std::time::Duration;

    const FUTURE_ID: VesperInterfaceId = VesperInterfaceId { bytes: [0xc1; 16] };

    struct AudioFixture;

    impl AudioProcessorFactory for AudioFixture {
        fn capabilities(&self) -> AudioProcessorCapabilities {
            AudioProcessorCapabilities {
                fixed_block: true,
                buffered: false,
            }
        }

        fn capabilities_json(&self) -> &str {
            r#"{"fixedBlock":true}"#
        }

        fn open(&self, _: VesperAudioFormat, _: &[u8]) -> PluginResult<OpenAudioProcessor> {
            Err(PluginError::Status(VESPER_ERR_UNSUPPORTED))
        }
    }

    unsafe extern "C" fn interface_at(
        _: *mut std::ffi::c_void,
        index: u32,
        out: *mut VesperInterfaceId,
    ) -> VesperStatus {
        let id = match index {
            0 => FUTURE_ID,
            1 => VESPER_AUDIO_PROCESSOR_ID,
            _ => return VESPER_ERR_INVALID_ARG,
        };
        // SAFETY: root enumeration supplies a writable interface ID.
        unsafe { out.write(id) };
        VESPER_OK
    }

    unsafe extern "C" fn entry() -> *const VesperPluginRoot {
        let root = player_plugin_abi::export::export_plugin(|| {
            Plugin::new("io.github.vesper.future", "Future fixture")
                .with_audio_processor(AudioFixture)
        });
        if !root.is_null() {
            // SAFETY: the export allocated and transferred a unique root owner.
            // The fixture extends the enumeration before publishing it to the loader.
            unsafe {
                let root = &mut *root.cast_mut();
                root.interface_count = 2;
                root.interface_at = Some(interface_at);
            }
        }
        root
    }

    #[test]
    fn preserves_unknown_uuid_without_rejecting_known_capabilities() {
        // SAFETY: the fixture callbacks are compiled into the test and stay resident.
        let plugin = unsafe { crate::native::NativePlugin::from_entry(entry) }.expect("valid root");
        let loaded =
            LoadedNativePlugin::from_native_plugin(plugin).expect("admit known capability");
        assert_eq!(
            loaded
                .unknown_interfaces()
                .map(|info| info.interface_id)
                .collect::<Vec<_>>(),
            [FUTURE_ID.bytes]
        );
        assert!(
            loaded
                .native_plugin()
                .audio_processor(Duration::from_millis(100))
                .is_ok()
        );
    }
}
