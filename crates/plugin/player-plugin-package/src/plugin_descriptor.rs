use std::collections::HashSet;

use player_plugin::{
    PLUGIN_CATALOG_MIGRATION_VERSION, PluginCatalogError, PluginProvision, PluginReference,
    PluginRequirement, PluginTransport, validate_plugin_provisions, validate_plugin_requirements,
};
use player_plugin_abi::{
    VESPER_MAX_CAPABILITY_INSTANCE_ID_BYTES, VESPER_PLUGIN_ABI_MAJOR, VESPER_PLUGIN_ABI_MINOR,
};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

const PLUGIN_DESCRIPTOR_SCHEMA_VERSION: u32 = 1;
const MAX_PLUGIN_NAME_BYTES: usize = 128;
const MAX_PLUGIN_DESCRIPTION_BYTES: usize = 1024;
const MAX_LICENSE_BYTES: usize = 128;
const MAX_HOST_SDK_REQUIREMENT_BYTES: usize = 128;
const MAX_CAPABILITIES: usize = 64;
const MAX_REDISTRIBUTION_ENTRIES: usize = 64;
const MAX_REDISTRIBUTION_COMPONENT_BYTES: usize = 128;
const MAX_REDISTRIBUTION_VALUE_BYTES: usize = 512;

/// Artifact-independent plugin identity and capability metadata.
///
/// Its canonical hash can be embedded inside an AAR or framework without
/// creating a cycle with the outer package manifest's artifact hashes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginDescriptor {
    pub schema_version: u32,
    pub plugin: PluginIdentityDescriptor,
    pub compatibility: PluginCompatibilityDescriptor,
    pub capabilities: Vec<PluginCapabilityDescriptor>,
    #[serde(default)]
    pub requires: Vec<PluginRequirement>,
    #[serde(default)]
    pub provides: Vec<PluginProvision>,
    #[serde(default)]
    pub redistribution: Vec<PluginRedistributionDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginIdentityDescriptor {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub license: String,
    pub publisher: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginCompatibilityDescriptor {
    pub host_sdk: String,
    pub abi_major: u16,
    pub abi_minor_min: u16,
    pub abi_minor_max: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginCapabilityDescriptor {
    pub interface_id: String,
    pub instance_id: String,
    pub interface_major: u16,
    pub interface_minor: u16,
    pub stability: PluginStability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginStability {
    Stable,
    Experimental,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginRedistributionDescriptor {
    pub component: String,
    pub license: String,
    pub notice: String,
    pub source: String,
    pub build_configuration: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relinking_materials: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalPluginDescriptor {
    descriptor: PluginDescriptor,
    json: Vec<u8>,
    sha256: String,
}

impl CanonicalPluginDescriptor {
    pub fn descriptor(&self) -> &PluginDescriptor {
        &self.descriptor
    }

    pub fn json(&self) -> &[u8] {
        &self.json
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[derive(Debug, Error)]
pub enum PluginDescriptorError {
    #[error("invalid vesper-plugin.toml: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("invalid plugin descriptor field `{field}`: {message}")]
    InvalidField { field: String, message: String },
    #[error("duplicate plugin capability `{interface_id}:{instance_id}`")]
    DuplicateCapability {
        interface_id: String,
        instance_id: String,
    },
    #[error(transparent)]
    Catalog(#[from] PluginCatalogError),
    #[error("failed to serialize canonical plugin descriptor: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PluginCompatibilityError {
    #[error(
        "plugin `{plugin_id}` does not support host SDK {actual}; required {required}; migration entry: {migration_version}"
    )]
    HostSdkMismatch {
        plugin_id: String,
        migration_version: String,
        required: String,
        actual: Version,
    },
    #[error(
        "plugin `{plugin_id}` requires ABI major {required}, but the host provides ABI major {actual}; migration entry: {migration_version}"
    )]
    AbiMajorMismatch {
        plugin_id: String,
        migration_version: String,
        required: u16,
        actual: u16,
    },
    #[error(
        "plugin `{plugin_id}` requires ABI minor {minimum}..={maximum}, but the host provides ABI minor {actual}; migration entry: {migration_version}"
    )]
    AbiMinorMismatch {
        plugin_id: String,
        migration_version: String,
        minimum: u16,
        maximum: u16,
        actual: u16,
    },
}

impl PluginDescriptor {
    pub fn from_toml(source: &str) -> Result<Self, PluginDescriptorError> {
        let descriptor: Self = toml::from_str(source)?;
        descriptor.validate()?;
        Ok(descriptor)
    }

    pub fn canonicalize(&self) -> Result<CanonicalPluginDescriptor, PluginDescriptorError> {
        self.validate()?;
        let mut descriptor = self.clone();
        descriptor.capabilities.sort_by(|left, right| {
            (&left.interface_id, &left.instance_id).cmp(&(&right.interface_id, &right.instance_id))
        });
        descriptor.requires.sort();
        descriptor.provides.sort();
        descriptor.redistribution.sort_by(|left, right| {
            (&left.component, &left.license).cmp(&(&right.component, &right.license))
        });
        let json = serde_json::to_vec(&descriptor)?;
        let sha256 = hex::encode(Sha256::digest(&json));
        Ok(CanonicalPluginDescriptor {
            descriptor,
            json,
            sha256,
        })
    }

    /// Evaluates this transport-neutral descriptor against one concrete host.
    pub fn evaluate_host_compatibility(
        &self,
        host_sdk: &Version,
        host_abi_major: u16,
        host_abi_minor: u16,
    ) -> Result<(), PluginCompatibilityError> {
        let requirement = VersionReq::parse(&self.compatibility.host_sdk).map_err(|_| {
            PluginCompatibilityError::HostSdkMismatch {
                plugin_id: self.plugin.id.clone(),
                migration_version: PLUGIN_CATALOG_MIGRATION_VERSION.to_owned(),
                required: self.compatibility.host_sdk.clone(),
                actual: host_sdk.clone(),
            }
        })?;
        if !requirement.matches(host_sdk) {
            return Err(PluginCompatibilityError::HostSdkMismatch {
                plugin_id: self.plugin.id.clone(),
                migration_version: PLUGIN_CATALOG_MIGRATION_VERSION.to_owned(),
                required: self.compatibility.host_sdk.clone(),
                actual: host_sdk.clone(),
            });
        }
        if self.compatibility.abi_major != host_abi_major {
            return Err(PluginCompatibilityError::AbiMajorMismatch {
                plugin_id: self.plugin.id.clone(),
                migration_version: PLUGIN_CATALOG_MIGRATION_VERSION.to_owned(),
                required: self.compatibility.abi_major,
                actual: host_abi_major,
            });
        }
        if !(self.compatibility.abi_minor_min..=self.compatibility.abi_minor_max)
            .contains(&host_abi_minor)
        {
            return Err(PluginCompatibilityError::AbiMinorMismatch {
                plugin_id: self.plugin.id.clone(),
                migration_version: PLUGIN_CATALOG_MIGRATION_VERSION.to_owned(),
                minimum: self.compatibility.abi_minor_min,
                maximum: self.compatibility.abi_minor_max,
                actual: host_abi_minor,
            });
        }
        Ok(())
    }

    pub fn evaluate_current_host_compatibility(
        &self,
        host_sdk: &Version,
    ) -> Result<(), PluginCompatibilityError> {
        self.evaluate_host_compatibility(host_sdk, VESPER_PLUGIN_ABI_MAJOR, VESPER_PLUGIN_ABI_MINOR)
    }

    pub fn validate(&self) -> Result<(), PluginDescriptorError> {
        if self.schema_version != PLUGIN_DESCRIPTOR_SCHEMA_VERSION {
            return invalid(
                "schema_version",
                format!(
                    "expected {PLUGIN_DESCRIPTOR_SCHEMA_VERSION}, got {}",
                    self.schema_version
                ),
            );
        }
        validate_identity("plugin.id", &self.plugin.id)?;
        validate_identity("plugin.publisher", &self.plugin.publisher)?;
        validate_text("plugin.name", &self.plugin.name, MAX_PLUGIN_NAME_BYTES)?;
        validate_text(
            "plugin.description",
            &self.plugin.description,
            MAX_PLUGIN_DESCRIPTION_BYTES,
        )?;
        validate_text("plugin.license", &self.plugin.license, MAX_LICENSE_BYTES)?;
        Version::parse(&self.plugin.version)
            .map_err(|error| field_error("plugin.version", error.to_string()))?;

        validate_text(
            "compatibility.host_sdk",
            &self.compatibility.host_sdk,
            MAX_HOST_SDK_REQUIREMENT_BYTES,
        )?;
        VersionReq::parse(&self.compatibility.host_sdk)
            .map_err(|error| field_error("compatibility.host_sdk", error.to_string()))?;
        if self.compatibility.abi_major == 0 {
            return invalid("compatibility.abi_major", "must be greater than zero");
        }
        if self.compatibility.abi_minor_min > self.compatibility.abi_minor_max {
            return invalid(
                "compatibility.abi_minor_min",
                "must not exceed compatibility.abi_minor_max",
            );
        }

        if self.capabilities.is_empty() || self.capabilities.len() > MAX_CAPABILITIES {
            return invalid(
                "capabilities",
                format!("must contain 1 to {MAX_CAPABILITIES} entries"),
            );
        }
        let mut capability_keys = HashSet::with_capacity(self.capabilities.len());
        for capability in &self.capabilities {
            let interface_id = Uuid::parse_str(&capability.interface_id)
                .map_err(|error| field_error("capabilities.interface_id", error.to_string()))?;
            if interface_id.hyphenated().to_string() != capability.interface_id {
                return invalid(
                    "capabilities.interface_id",
                    "must use canonical lowercase hyphenated UUID form",
                );
            }
            validate_identity("capabilities.instance_id", &capability.instance_id)?;
            if capability.instance_id.len() > VESPER_MAX_CAPABILITY_INSTANCE_ID_BYTES {
                return invalid(
                    "capabilities.instance_id",
                    format!(
                        "must not exceed {VESPER_MAX_CAPABILITY_INSTANCE_ID_BYTES} UTF-8 bytes"
                    ),
                );
            }
            if capability.interface_major == 0 {
                return invalid("capabilities.interface_major", "must be greater than zero");
            }
            if !capability_keys.insert((&capability.interface_id, &capability.instance_id)) {
                return Err(PluginDescriptorError::DuplicateCapability {
                    interface_id: capability.interface_id.clone(),
                    instance_id: capability.instance_id.clone(),
                });
            }
        }

        validate_plugin_requirements(&self.requires)?;
        validate_plugin_provisions(&self.provides)?;

        if self.redistribution.len() > MAX_REDISTRIBUTION_ENTRIES {
            return invalid(
                "redistribution",
                format!("must contain at most {MAX_REDISTRIBUTION_ENTRIES} entries"),
            );
        }
        for entry in &self.redistribution {
            validate_text(
                "redistribution.component",
                &entry.component,
                MAX_REDISTRIBUTION_COMPONENT_BYTES,
            )?;
            validate_text("redistribution.license", &entry.license, MAX_LICENSE_BYTES)?;
            validate_text(
                "redistribution.notice",
                &entry.notice,
                MAX_REDISTRIBUTION_VALUE_BYTES,
            )?;
            validate_text(
                "redistribution.source",
                &entry.source,
                MAX_REDISTRIBUTION_VALUE_BYTES,
            )?;
            validate_text(
                "redistribution.build_configuration",
                &entry.build_configuration,
                MAX_REDISTRIBUTION_VALUE_BYTES,
            )?;
            if let Some(relinking_materials) = entry.relinking_materials.as_deref() {
                validate_text(
                    "redistribution.relinking_materials",
                    relinking_materials,
                    MAX_REDISTRIBUTION_VALUE_BYTES,
                )?;
            }
            if let Some(profile_hash) = entry.profile_hash.as_deref() {
                validate_sha256("redistribution.profile_hash", profile_hash)?;
            }
        }
        Ok(())
    }
}

fn validate_identity(field: &str, value: &str) -> Result<(), PluginDescriptorError> {
    PluginReference::new(value, None, PluginTransport::Native)
        .map(|_| ())
        .map_err(|error| field_error(field, error.to_string()))
}

fn validate_text(
    field: &str,
    value: &str,
    maximum_bytes: usize,
) -> Result<(), PluginDescriptorError> {
    if value.is_empty() || value.len() > maximum_bytes {
        return invalid(
            field,
            format!("must contain 1 to {maximum_bytes} UTF-8 bytes"),
        );
    }
    Ok(())
}

fn validate_sha256(field: &str, value: &str) -> Result<(), PluginDescriptorError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return invalid(field, "must be 64 lowercase hexadecimal characters");
    }
    Ok(())
}

fn invalid<T>(field: &str, message: impl Into<String>) -> Result<T, PluginDescriptorError> {
    Err(field_error(field, message))
}

fn field_error(field: &str, message: impl Into<String>) -> PluginDescriptorError {
    PluginDescriptorError::InvalidField {
        field: field.to_owned(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_checkout_root() -> Option<std::path::PathBuf> {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .ok()?;
        let root = manifest_dir.join("../../..");
        let workspace_member = root
            .join("crates/plugin/player-plugin-package")
            .canonicalize()
            .ok()?;
        (manifest_dir == workspace_member).then_some(root)
    }

    fn descriptor_toml(capabilities: &str) -> String {
        format!(
            r#"
schema_version = 1

[plugin]
id = "dev.vesper.fixture"
name = "Fixture"
version = "1.2.3"
description = "Fixture plugin"
license = "Apache-2.0"
publisher = "dev.vesper.publisher"

[compatibility]
host_sdk = ">=0.4.0, <0.5.0"
abi_major = 1
abi_minor_min = 0
abi_minor_max = 0

{capabilities}
"#
        )
    }

    fn capability(instance_id: &str) -> String {
        format!(
            r#"
[[capabilities]]
interface_id = "e9479dbc-42d2-575e-b39e-a24bc512fbc7"
instance_id = "{instance_id}"
interface_major = 1
interface_minor = 0
stability = "stable"
"#
        )
    }

    #[test]
    fn canonical_descriptor_is_stable_across_capability_order() {
        let first = capability("dev.vesper.fixture.first");
        let second = capability("dev.vesper.fixture.second");
        let left = PluginDescriptor::from_toml(&descriptor_toml(&format!("{second}{first}")))
            .expect("valid descriptor")
            .canonicalize()
            .expect("canonical descriptor");
        let right = PluginDescriptor::from_toml(&descriptor_toml(&format!("{first}{second}")))
            .expect("valid descriptor")
            .canonicalize()
            .expect("canonical descriptor");

        assert_eq!(left.json(), right.json());
        assert_eq!(left.sha256(), right.sha256());
        assert_eq!(left.sha256().len(), 64);
        assert!(left.json().starts_with(b"{\"schema_version\":1,"));
    }

    #[test]
    fn canonical_descriptor_is_stable_across_requirement_and_provision_order() {
        let first = r#"
[[requires]]
service = "dev.vesper.service.audio"
requirement = ">=1.0.0"

[[provides]]
service = "dev.vesper.service.audio"
version = "1.2.0"
"#;
        let second = r#"
[[requires]]
service = "dev.vesper.service.video"
requirement = ">=2.0.0"

[[provides]]
service = "dev.vesper.service.video"
version = "2.3.0"
"#;
        let left = PluginDescriptor::from_toml(&format!(
            "{}{}{}",
            descriptor_toml(&capability("dev.vesper.fixture.primary")),
            first,
            second
        ))
        .expect("valid descriptor")
        .canonicalize()
        .expect("canonical descriptor");
        let right = PluginDescriptor::from_toml(&format!(
            "{}{}{}",
            descriptor_toml(&capability("dev.vesper.fixture.primary")),
            second,
            first
        ))
        .expect("valid descriptor")
        .canonicalize()
        .expect("canonical descriptor");

        assert_eq!(left.json(), right.json());
        assert_eq!(left.sha256(), right.sha256());
    }

    #[test]
    fn descriptor_rejects_unknown_fields_and_lossy_identity_forms() {
        let source = descriptor_toml(&capability("dev.vesper.fixture.primary"));
        let unknown = source.replace(
            "name = \"Fixture\"",
            "name = \"Fixture\"\nunexpected = true",
        );
        assert!(matches!(
            PluginDescriptor::from_toml(&unknown),
            Err(PluginDescriptorError::Toml(_))
        ));

        let invalid_identity = source.replace("dev.vesper.fixture", " Dev.Vesper.Fixture ");
        assert!(matches!(
            PluginDescriptor::from_toml(&invalid_identity),
            Err(PluginDescriptorError::InvalidField { ref field, .. }) if field == "plugin.id"
        ));
    }

    #[test]
    fn descriptor_rejects_duplicate_capabilities_and_invalid_abi_ranges() {
        let duplicate = capability("dev.vesper.fixture.primary");
        assert!(matches!(
            PluginDescriptor::from_toml(&descriptor_toml(&format!("{duplicate}{duplicate}"))),
            Err(PluginDescriptorError::DuplicateCapability { .. })
        ));

        let invalid_range = descriptor_toml(&capability("dev.vesper.fixture.primary"))
            .replace("abi_minor_min = 0", "abi_minor_min = 2");
        assert!(matches!(
            PluginDescriptor::from_toml(&invalid_range),
            Err(PluginDescriptorError::InvalidField { ref field, .. })
                if field == "compatibility.abi_minor_min"
        ));
    }

    #[test]
    fn schema_validation_preserves_future_abi_for_host_compatibility_evaluation() {
        let source = descriptor_toml(&capability("dev.vesper.fixture.primary"))
            .replace("abi_major = 1", "abi_major = 2");
        let descriptor = PluginDescriptor::from_toml(&source).expect("valid future descriptor");
        descriptor
            .canonicalize()
            .expect("future descriptor remains canonicalizable");

        assert_eq!(
            descriptor.evaluate_current_host_compatibility(&Version::new(0, 4, 0)),
            Err(PluginCompatibilityError::AbiMajorMismatch {
                plugin_id: "dev.vesper.fixture".to_owned(),
                migration_version: PLUGIN_CATALOG_MIGRATION_VERSION.to_owned(),
                required: 2,
                actual: VESPER_PLUGIN_ABI_MAJOR,
            })
        );
    }

    #[test]
    fn descriptor_abi_major_matches_the_nonzero_u16_wire_contract() {
        let source = descriptor_toml(&capability("dev.vesper.fixture.primary"));

        let zero = source.replace("abi_major = 1", "abi_major = 0");
        assert!(matches!(
            PluginDescriptor::from_toml(&zero),
            Err(PluginDescriptorError::InvalidField { ref field, .. })
                if field == "compatibility.abi_major"
        ));

        let maximum = source.replace("abi_major = 1", "abi_major = 65535");
        assert_eq!(
            PluginDescriptor::from_toml(&maximum)
                .expect("u16 maximum ABI major")
                .compatibility
                .abi_major,
            u16::MAX
        );

        let overflow = source.replace("abi_major = 1", "abi_major = 65536");
        assert!(matches!(
            PluginDescriptor::from_toml(&overflow),
            Err(PluginDescriptorError::Toml(_))
        ));
    }

    #[test]
    fn public_schemas_match_the_nonzero_u16_abi_major_contract() {
        // Public schemas intentionally live outside Rust crates. Keep the
        // repository drift check without making packaged crate tests depend on
        // files that Cargo cannot include from the workspace root.
        let Some(workspace) = source_checkout_root() else {
            return;
        };
        for relative_path in [
            "schemas/vesper-plugin/project.schema.json",
            "schemas/vesper-plugin/manifest.schema.json",
            "schemas/vesper-plugin/descriptor.schema.json",
        ] {
            let path = workspace.join(relative_path);
            let bytes = std::fs::read(&path).expect("read plugin schema");
            let schema: serde_json::Value =
                serde_json::from_slice(&bytes).expect("parse plugin schema");
            let abi_major = &schema["$defs"]["compatibility"]["properties"]["abi_major"];

            assert_eq!(abi_major["type"], "integer", "{relative_path}");
            assert_eq!(abi_major["minimum"], 1, "{relative_path}");
            assert_eq!(abi_major["maximum"], u16::MAX, "{relative_path}");
            assert!(abi_major.get("const").is_none(), "{relative_path}");
        }
    }

    #[test]
    fn public_descriptor_schema_matches_canonical_redistribution_contract() {
        let Some(workspace) = source_checkout_root() else {
            return;
        };
        let relative_path = "schemas/vesper-plugin/descriptor.schema.json";
        let bytes = std::fs::read(workspace.join(relative_path)).expect("read descriptor schema");
        let schema: serde_json::Value =
            serde_json::from_slice(&bytes).expect("parse descriptor schema");
        let required = schema["required"]
            .as_array()
            .expect("descriptor required fields");
        let redistribution = &schema["properties"]["redistribution"];

        assert!(
            required.iter().any(|field| field == "redistribution"),
            "{relative_path}"
        );
        assert!(
            required.iter().any(|field| field == "requires"),
            "{relative_path}"
        );
        assert!(
            required.iter().any(|field| field == "provides"),
            "{relative_path}"
        );
        assert_eq!(redistribution["type"], "array", "{relative_path}");
        assert_eq!(
            redistribution["maxItems"], MAX_REDISTRIBUTION_ENTRIES,
            "{relative_path}"
        );
    }

    #[test]
    fn project_schema_keeps_dependency_arrays_optional_for_author_input() {
        let Some(workspace) = source_checkout_root() else {
            return;
        };
        let bytes = std::fs::read(workspace.join("schemas/vesper-plugin/project.schema.json"))
            .expect("read project schema");
        let schema: serde_json::Value =
            serde_json::from_slice(&bytes).expect("parse project schema");
        let required = schema["required"]
            .as_array()
            .expect("project required fields");
        assert!(!required.iter().any(|field| field == "requires"));
        assert!(!required.iter().any(|field| field == "provides"));
        assert_eq!(
            schema["properties"]["requires"]["default"],
            serde_json::json!([])
        );
        assert_eq!(
            schema["properties"]["provides"]["default"],
            serde_json::json!([])
        );
    }

    #[test]
    fn public_project_and_package_schemas_require_artifact_capability_references() {
        let Some(workspace) = source_checkout_root() else {
            return;
        };
        for (relative_path, artifact_definition) in [
            (
                "schemas/vesper-plugin/project.schema.json",
                "artifactSource",
            ),
            ("schemas/vesper-plugin/manifest.schema.json", "artifact"),
        ] {
            let path = workspace.join(relative_path);
            let bytes = std::fs::read(&path).expect("read plugin schema");
            let schema: serde_json::Value =
                serde_json::from_slice(&bytes).expect("parse plugin schema");
            let artifact = &schema["$defs"][artifact_definition];
            let required = artifact["required"]
                .as_array()
                .expect("artifact required fields");
            let capabilities = &artifact["properties"]["capabilities"];
            let capability_reference = &schema["$defs"]["artifactCapability"];

            assert!(
                required.iter().any(|field| field == "capabilities"),
                "{relative_path}"
            );
            assert_eq!(capabilities["type"], "array", "{relative_path}");
            assert_eq!(capabilities["minItems"], 1, "{relative_path}");
            assert_eq!(
                capabilities["maxItems"], MAX_CAPABILITIES,
                "{relative_path}"
            );
            assert_eq!(
                capabilities["items"]["$ref"], "#/$defs/artifactCapability",
                "{relative_path}"
            );
            assert_eq!(
                capability_reference["additionalProperties"], false,
                "{relative_path}"
            );
            assert_eq!(
                capability_reference["required"],
                serde_json::json!(["interface_id", "instance_id"]),
                "{relative_path}"
            );
        }
    }

    #[test]
    fn host_compatibility_checks_sdk_and_abi_minor_range_without_rewriting_them() {
        let descriptor = PluginDescriptor::from_toml(&descriptor_toml(&capability(
            "dev.vesper.fixture.primary",
        )))
        .expect("descriptor");

        assert!(matches!(
            descriptor.evaluate_current_host_compatibility(&Version::new(0, 5, 0)),
            Err(PluginCompatibilityError::HostSdkMismatch { .. })
        ));
        assert_eq!(
            descriptor.evaluate_host_compatibility(&Version::new(0, 4, 0), 1, 1),
            Err(PluginCompatibilityError::AbiMinorMismatch {
                plugin_id: "dev.vesper.fixture".to_owned(),
                migration_version: PLUGIN_CATALOG_MIGRATION_VERSION.to_owned(),
                minimum: 0,
                maximum: 0,
                actual: 1,
            })
        );
    }
}
