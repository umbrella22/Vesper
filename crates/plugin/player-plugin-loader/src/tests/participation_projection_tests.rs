use crate::{PluginCapabilityAvailability, PluginDiagnosticStatus};

#[test]
fn inspection_status_projects_only_capability_availability() {
    for status in [
        PluginDiagnosticStatus::DecoderSupported,
        PluginDiagnosticStatus::FrameProcessorSupported,
        PluginDiagnosticStatus::SourceNormalizerSupported,
    ] {
        assert_eq!(
            status.capability_availability(),
            PluginCapabilityAvailability::Available
        );
        assert_eq!(status.capability_availability().wire_name(), "available");
    }

    assert_eq!(
        PluginDiagnosticStatus::Loaded.capability_availability(),
        PluginCapabilityAvailability::Unknown
    );
    for status in [
        PluginDiagnosticStatus::LoadFailed,
        PluginDiagnosticStatus::UnsupportedKind,
        PluginDiagnosticStatus::DecoderUnsupported,
        PluginDiagnosticStatus::FrameProcessorUnsupported,
        PluginDiagnosticStatus::SourceNormalizerUnsupported,
    ] {
        assert_eq!(
            status.capability_availability(),
            PluginCapabilityAvailability::Unavailable
        );
    }
}
