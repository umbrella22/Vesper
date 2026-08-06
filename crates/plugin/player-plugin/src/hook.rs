use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    MAX_PLUGIN_DIAGNOSTICS, MAX_PLUGIN_ERROR_MESSAGE_BYTES, MAX_PLUGIN_EVENT_ID_BYTES,
    MAX_PLUGIN_EVENT_NAME_BYTES, MAX_PLUGIN_MEASUREMENTS, MAX_PLUGIN_PLATFORM_BYTES,
    MAX_PLUGIN_PROTOCOL_BYTES, MAX_PLUGIN_RESOURCE_IDENTITY_BYTES, MAX_PLUGIN_THREAD_BYTES,
    PluginDiagnostic, PluginMeasurement, PluginProtocolViolation,
    protocol::{validate_attributes, validate_optional_text, validate_text},
};

/// One bounded, transport-neutral event emitted by a host pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineEvent {
    pub run_id: String,
    pub session_id: String,
    pub platform: String,
    pub protocol: Option<String>,
    pub event_name: String,
    pub timestamp_ns: u64,
    pub thread: Option<String>,
    pub resource_identity: Option<String>,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    pub diagnostic: Option<PluginDiagnostic>,
}

impl PipelineEvent {
    pub fn validate(&self) -> Result<(), PipelineEventHookError> {
        validate_text(
            "pipeline_event.run_id",
            &self.run_id,
            MAX_PLUGIN_EVENT_ID_BYTES,
        )?;
        validate_text(
            "pipeline_event.session_id",
            &self.session_id,
            MAX_PLUGIN_EVENT_ID_BYTES,
        )?;
        validate_text(
            "pipeline_event.platform",
            &self.platform,
            MAX_PLUGIN_PLATFORM_BYTES,
        )?;
        validate_optional_text(
            "pipeline_event.protocol",
            self.protocol.as_deref(),
            MAX_PLUGIN_PROTOCOL_BYTES,
        )?;
        validate_text(
            "pipeline_event.event_name",
            &self.event_name,
            MAX_PLUGIN_EVENT_NAME_BYTES,
        )?;
        validate_optional_text(
            "pipeline_event.thread",
            self.thread.as_deref(),
            MAX_PLUGIN_THREAD_BYTES,
        )?;
        validate_optional_text(
            "pipeline_event.resource_identity",
            self.resource_identity.as_deref(),
            MAX_PLUGIN_RESOURCE_IDENTITY_BYTES,
        )?;
        validate_attributes(&self.attributes)?;
        if let Some(diagnostic) = &self.diagnostic {
            diagnostic.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineEventHookOutcome {
    pub accepted: bool,
    #[serde(default)]
    pub measurements: Vec<PluginMeasurement>,
    #[serde(default)]
    pub diagnostics: Vec<PluginDiagnostic>,
}

impl PipelineEventHookOutcome {
    pub fn accepted() -> Self {
        Self {
            accepted: true,
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<(), PipelineEventHookError> {
        if self.measurements.len() > MAX_PLUGIN_MEASUREMENTS {
            return Err(PipelineEventHookError::ProtocolViolation(format!(
                "event hook returned more than {MAX_PLUGIN_MEASUREMENTS} measurements"
            )));
        }
        if self.diagnostics.len() > MAX_PLUGIN_DIAGNOSTICS {
            return Err(PipelineEventHookError::ProtocolViolation(format!(
                "event hook returned more than {MAX_PLUGIN_DIAGNOSTICS} diagnostics"
            )));
        }
        for measurement in &self.measurements {
            measurement.validate()?;
        }
        for diagnostic in &self.diagnostics {
            diagnostic.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "code", content = "message")]
pub enum PipelineEventHookError {
    #[error("event hook rejected invalid input: {0}")]
    InvalidInput(String),
    #[error("payload codec error: {0}")]
    PayloadCodec(String),
    #[error("plugin ABI violation: {0}")]
    AbiViolation(String),
    #[error("event hook rejected the event: {0}")]
    Rejected(String),
    #[error("event hook failed: {0}")]
    Failed(String),
    #[error("event hook protocol violation: {0}")]
    ProtocolViolation(String),
}

impl PipelineEventHookError {
    pub fn validate_author_failure(&self) -> Result<(), Self> {
        let message = match self {
            Self::InvalidInput(message) | Self::Rejected(message) | Self::Failed(message) => {
                message
            }
            Self::PayloadCodec(_) | Self::AbiViolation(_) | Self::ProtocolViolation(_) => {
                return Err(Self::ProtocolViolation(
                    "plugin returned a host-owned event-hook error kind".to_owned(),
                ));
            }
        };
        validate_text(
            "event_hook.error.message",
            message,
            MAX_PLUGIN_ERROR_MESSAGE_BYTES,
        )
        .map_err(Self::from)
    }
}

impl From<PluginProtocolViolation> for PipelineEventHookError {
    fn from(value: PluginProtocolViolation) -> Self {
        Self::ProtocolViolation(value.to_string())
    }
}

pub trait PipelineEventHook: Send + Sync {
    fn on_event(
        &self,
        event: &PipelineEvent,
    ) -> Result<PipelineEventHookOutcome, PipelineEventHookError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> PipelineEvent {
        PipelineEvent {
            run_id: "run-1".to_owned(),
            session_id: "session-1".to_owned(),
            platform: "test".to_owned(),
            protocol: Some("hls".to_owned()),
            event_name: "download.completed".to_owned(),
            timestamp_ns: 1,
            thread: None,
            resource_identity: Some("download-task:1".to_owned()),
            attributes: BTreeMap::new(),
            diagnostic: None,
        }
    }

    #[test]
    fn event_validates_transport_fields_and_structured_diagnostic() {
        let mut event = event();
        assert_eq!(event.validate(), Ok(()));

        event.platform.clear();
        assert!(matches!(
            event.validate(),
            Err(PipelineEventHookError::ProtocolViolation(_))
        ));

        event.platform = "test".to_owned();
        event.diagnostic = Some(PluginDiagnostic {
            code: "x".repeat(65),
            severity: crate::PluginDiagnosticSeverity::Error,
            message: "invalid diagnostic".to_owned(),
            attributes: BTreeMap::new(),
        });
        assert!(matches!(
            event.validate(),
            Err(PipelineEventHookError::ProtocolViolation(_))
        ));
    }

    #[test]
    fn event_accepts_awkward_valid_utf8_without_rewriting_identity() {
        let mut event = event();
        event.resource_identity = Some("opaque:资源 identity/with spaces".to_owned());
        assert_eq!(event.validate(), Ok(()));
        assert_eq!(
            event.resource_identity.as_deref(),
            Some("opaque:资源 identity/with spaces")
        );
    }

    #[test]
    fn outcome_rejects_non_finite_measurement() {
        let outcome = PipelineEventHookOutcome {
            accepted: true,
            measurements: vec![PluginMeasurement {
                name: "latency".to_owned(),
                value: f64::INFINITY,
                unit: "ms".to_owned(),
                attributes: Default::default(),
            }],
            diagnostics: Vec::new(),
        };
        assert!(matches!(
            outcome.validate(),
            Err(PipelineEventHookError::ProtocolViolation(_))
        ));
    }
}
