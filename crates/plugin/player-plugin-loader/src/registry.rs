use super::*;
use crate::diagnostics::{
    decoder_capability_summary, source_normalizer_packet_capability_summary,
    source_normalizer_resource_capability_summary,
};

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
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PluginRegistry {
    records: Vec<PluginDiagnosticRecord>,
}

impl PluginRegistry {
    pub fn inspect_decoder_support(
        paths: impl IntoIterator<Item = impl AsRef<Path>>,
        request: DecoderPluginMatchRequest,
    ) -> Self {
        let records = paths
            .into_iter()
            .map(|path| {
                let path = path.as_ref().to_path_buf();
                match LoadedDynamicPlugin::load(&path) {
                    Ok(plugin) => {
                        PluginDiagnosticRecord::from_loaded_plugin(path, &plugin, Some(&request))
                    }
                    Err(error) => PluginDiagnosticRecord::load_failed(path, error),
                }
            })
            .collect();
        Self { records }
    }

    pub fn inspect_frame_processor_support(
        paths: impl IntoIterator<Item = impl AsRef<Path>>,
    ) -> Self {
        let records = paths
            .into_iter()
            .map(|path| {
                let path = path.as_ref().to_path_buf();
                match LoadedDynamicPlugin::load(&path) {
                    Ok(plugin) => {
                        PluginDiagnosticRecord::from_loaded_frame_processor_plugin(path, &plugin)
                    }
                    Err(error) => PluginDiagnosticRecord::load_failed(path, error),
                }
            })
            .collect();
        Self { records }
    }

    pub fn inspect_source_normalizer_support(
        paths: impl IntoIterator<Item = impl AsRef<Path>>,
    ) -> Self {
        let records = paths
            .into_iter()
            .map(|path| {
                let path = path.as_ref().to_path_buf();
                match LoadedDynamicPlugin::load(&path) {
                    Ok(plugin) => {
                        PluginDiagnosticRecord::from_loaded_source_normalizer_plugin(path, &plugin)
                    }
                    Err(error) => PluginDiagnosticRecord::load_failed(path, error),
                }
            })
            .collect();
        Self { records }
    }

    pub fn from_records(records: Vec<PluginDiagnosticRecord>) -> Self {
        Self { records }
    }

    pub fn records(&self) -> &[PluginDiagnosticRecord] {
        &self.records
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

    pub fn supports_decoder(&self, request: &DecoderPluginMatchRequest) -> bool {
        self.best_decoder_for(request).is_some()
    }

    pub fn supports_native_decoder(&self, request: &DecoderPluginMatchRequest) -> bool {
        self.best_native_decoder_for(request).is_some()
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

    pub fn best_source_normalizer_resource(&self) -> Option<&PluginDiagnosticRecord> {
        self.records.iter().find(|record| {
            record.status == PluginDiagnosticStatus::SourceNormalizerSupported
                && source_normalizer_resource_capability_summary(record).is_some()
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
