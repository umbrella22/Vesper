use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAX_PLUGIN_ATTRIBUTES: usize = 32;
pub const MAX_PLUGIN_ATTRIBUTE_KEY_BYTES: usize = 64;
pub const MAX_PLUGIN_ATTRIBUTE_VALUE_BYTES: usize = 256;
pub const MAX_PLUGIN_DIAGNOSTIC_MESSAGE_BYTES: usize = 256;
pub const MAX_PLUGIN_ERROR_MESSAGE_BYTES: usize = 256;
pub const MAX_PLUGIN_MEASUREMENTS: usize = 128;
pub const MAX_PLUGIN_DIAGNOSTICS: usize = 64;
pub const MAX_PLUGIN_EVENT_ID_BYTES: usize = 128;
pub const MAX_PLUGIN_EVENT_NAME_BYTES: usize = 64;
pub const MAX_PLUGIN_PLATFORM_BYTES: usize = 64;
pub const MAX_PLUGIN_PROTOCOL_BYTES: usize = 64;
pub const MAX_PLUGIN_THREAD_BYTES: usize = 128;
pub const MAX_PLUGIN_RESOURCE_IDENTITY_BYTES: usize = 256;
/// Maximum encoded size of one pipeline event at a plugin transport boundary.
pub const MAX_PIPELINE_EVENT_INPUT_BYTES: usize = 256 * 1024;
/// Maximum byte length of one source-normalizer packet payload.
pub const MAX_SOURCE_NORMALIZER_PACKET_BYTES: usize =
    player_plugin_abi::VESPER_MAX_PACKET_BYTES as usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginDiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDiagnostic {
    pub code: String,
    pub severity: PluginDiagnosticSeverity,
    pub message: String,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

impl PluginDiagnostic {
    pub fn validate(&self) -> Result<(), PluginProtocolViolation> {
        validate_text(
            "diagnostic.code",
            &self.code,
            MAX_PLUGIN_ATTRIBUTE_KEY_BYTES,
        )?;
        validate_text(
            "diagnostic.message",
            &self.message,
            MAX_PLUGIN_DIAGNOSTIC_MESSAGE_BYTES,
        )?;
        validate_attributes(&self.attributes)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginMeasurement {
    pub name: String,
    pub value: f64,
    pub unit: String,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

impl PluginMeasurement {
    pub fn validate(&self) -> Result<(), PluginProtocolViolation> {
        validate_text(
            "measurement.name",
            &self.name,
            MAX_PLUGIN_ATTRIBUTE_KEY_BYTES,
        )?;
        validate_text(
            "measurement.unit",
            &self.unit,
            MAX_PLUGIN_ATTRIBUTE_KEY_BYTES,
        )?;
        if !self.value.is_finite() {
            return Err(PluginProtocolViolation::NonFiniteMeasurement {
                name: self.name.clone(),
            });
        }
        validate_attributes(&self.attributes)
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PluginProtocolViolation {
    #[error("{field} must not be empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds {limit} bytes")]
    TooLong { field: &'static str, limit: usize },
    #[error("plugin attributes exceed the {limit}-entry protocol limit")]
    TooManyAttributes { limit: usize },
    #[error("measurement `{name}` is not finite")]
    NonFiniteMeasurement { name: String },
}

pub(crate) fn validate_attributes(
    attributes: &BTreeMap<String, String>,
) -> Result<(), PluginProtocolViolation> {
    if attributes.len() > MAX_PLUGIN_ATTRIBUTES {
        return Err(PluginProtocolViolation::TooManyAttributes {
            limit: MAX_PLUGIN_ATTRIBUTES,
        });
    }
    for (key, value) in attributes {
        validate_text("attribute.key", key, MAX_PLUGIN_ATTRIBUTE_KEY_BYTES)?;
        validate_text("attribute.value", value, MAX_PLUGIN_ATTRIBUTE_VALUE_BYTES)?;
    }
    Ok(())
}

pub(crate) fn validate_text(
    field: &'static str,
    value: &str,
    limit: usize,
) -> Result<(), PluginProtocolViolation> {
    if value.is_empty() {
        return Err(PluginProtocolViolation::Empty { field });
    }
    if value.len() > limit {
        return Err(PluginProtocolViolation::TooLong { field, limit });
    }
    Ok(())
}

pub(crate) fn validate_optional_text(
    field: &'static str,
    value: Option<&str>,
    limit: usize,
) -> Result<(), PluginProtocolViolation> {
    if let Some(value) = value {
        validate_text(field, value, limit)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measurements_reject_non_finite_values() {
        let measurement = PluginMeasurement {
            name: "startup".to_owned(),
            value: f64::NAN,
            unit: "ms".to_owned(),
            attributes: BTreeMap::new(),
        };
        assert!(matches!(
            measurement.validate(),
            Err(PluginProtocolViolation::NonFiniteMeasurement { .. })
        ));
    }

    #[test]
    fn diagnostics_enforce_named_protocol_limits() {
        let diagnostic = PluginDiagnostic {
            code: "event.accepted".to_owned(),
            severity: PluginDiagnosticSeverity::Info,
            message: "accepted".to_owned(),
            attributes: (0..=MAX_PLUGIN_ATTRIBUTES)
                .map(|index| (format!("key-{index}"), "value".to_owned()))
                .collect(),
        };
        assert_eq!(
            diagnostic.validate(),
            Err(PluginProtocolViolation::TooManyAttributes {
                limit: MAX_PLUGIN_ATTRIBUTES
            })
        );
    }
}
