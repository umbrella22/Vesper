use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use player_plugin_abi::{VESPER_MAX_CAPABILITY_INSTANCE_ID_BYTES, VESPER_MAX_PLUGIN_ID_BYTES};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginTransport {
    Native,
    Wasm,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PluginReferenceError {
    #[error("plugin_id must be a valid reverse-DNS identity")]
    InvalidPluginId,
    #[error("capability_instance_id must be a valid reverse-DNS identity")]
    InvalidCapabilityInstanceId,
}

/// Explicit selection of one plugin and transport.
///
/// Omitting `capability_instance_id` asks the registry to select the only
/// implementation of the requested interface. Zero or multiple matches are
/// errors; selection never falls back to another transport.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginReference {
    plugin_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    capability_instance_id: Option<String>,
    transport: PluginTransport,
}

impl PluginReference {
    pub fn new(
        plugin_id: impl Into<String>,
        capability_instance_id: Option<String>,
        transport: PluginTransport,
    ) -> Result<Self, PluginReferenceError> {
        let plugin_id = plugin_id.into();
        if !is_reverse_dns(&plugin_id, VESPER_MAX_PLUGIN_ID_BYTES) {
            return Err(PluginReferenceError::InvalidPluginId);
        }
        if let Some(instance_id) = capability_instance_id.as_deref()
            && !is_reverse_dns(instance_id, VESPER_MAX_CAPABILITY_INSTANCE_ID_BYTES)
        {
            return Err(PluginReferenceError::InvalidCapabilityInstanceId);
        }
        Ok(Self {
            plugin_id,
            capability_instance_id,
            transport,
        })
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn capability_instance_id(&self) -> Option<&str> {
        self.capability_instance_id.as_deref()
    }

    pub const fn transport(&self) -> PluginTransport {
        self.transport
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginReferenceWire {
    plugin_id: String,
    #[serde(default)]
    capability_instance_id: Option<String>,
    transport: PluginTransport,
}

impl<'de> Deserialize<'de> for PluginReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = PluginReferenceWire::deserialize(deserializer)?;
        Self::new(wire.plugin_id, wire.capability_instance_id, wire.transport)
            .map_err(serde::de::Error::custom)
    }
}

pub(crate) fn is_reverse_dns(value: &str, max_bytes: usize) -> bool {
    if value.is_empty() || value.len() > max_bytes || !value.is_ascii() {
        return false;
    }
    let mut segments = value.split('.');
    let Some(first) = segments.next() else {
        return false;
    };
    let Some(second) = segments.next() else {
        return false;
    };
    valid_segment(first) && valid_segment(second) && segments.all(valid_segment)
}

fn valid_segment(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    matches!(bytes.first(), Some(b'a'..=b'z'))
        && matches!(bytes.last(), Some(b'a'..=b'z' | b'0'..=b'9'))
        && bytes
            .iter()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_requires_explicit_transport_and_preserves_identity() {
        let reference = PluginReference::new(
            "dev.vesper.example-plugin",
            Some("dev.vesper.example-plugin.primary".to_owned()),
            PluginTransport::Wasm,
        )
        .expect("valid reference");
        let encoded = serde_json::to_value(&reference).expect("serialize reference");
        assert_eq!(encoded["pluginId"], "dev.vesper.example-plugin");
        assert_eq!(encoded["transport"], "wasm");
        assert_eq!(
            serde_json::from_value::<PluginReference>(encoded).expect("deserialize reference"),
            reference
        );
    }

    #[test]
    fn reference_rejects_lossy_or_ambiguous_identity_forms() {
        for invalid in [
            "Vesper.Plugin",
            "vesper",
            "dev..plugin",
            "dev.plugin_1",
            "dev.plugin/../other",
            "开发.插件",
        ] {
            assert_eq!(
                PluginReference::new(invalid, None, PluginTransport::Native),
                Err(PluginReferenceError::InvalidPluginId),
                "invalid identity {invalid}"
            );
        }
    }

    #[test]
    fn deserialization_cannot_bypass_validation() {
        let error = serde_json::from_str::<PluginReference>(
            r#"{"pluginId":"invalid","transport":"native"}"#,
        )
        .expect_err("invalid reference");
        assert!(error.to_string().contains("reverse-DNS"));
    }
}
