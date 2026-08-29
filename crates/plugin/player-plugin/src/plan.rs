//! Immutable, canonical plugin resolution plans.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    PluginArtifactTransport, PluginCatalog, PluginCatalogError, PluginCatalogRecord,
    PluginProvision, PluginRequirement, PluginResolution, PluginResolutionError, PluginResolver,
    PluginResolverPolicy, validate_plugin_provisions, validate_plugin_requirements,
};

/// Version of the immutable plugin plan wire model.
pub const PLUGIN_PLAN_SCHEMA_VERSION: u32 = 1;
/// Maximum selected providers implied by the resolver's constraint budget.
pub const MAX_PLUGIN_PLAN_PROVIDERS: usize = crate::MAX_PLUGIN_RESOLUTION_CONSTRAINTS;

/// Immutable host policy snapshot bound into a plugin plan fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginPlanPolicy {
    transport: PluginArtifactTransport,
    target: String,
    architecture: String,
    abi_major: u16,
    abi_minor: u16,
    plugin_priorities: BTreeMap<String, i32>,
}

impl PluginPlanPolicy {
    pub const fn transport(&self) -> PluginArtifactTransport {
        self.transport
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn architecture(&self) -> &str {
        &self.architecture
    }

    pub const fn abi_major(&self) -> u16 {
        self.abi_major
    }

    pub const fn abi_minor(&self) -> u16 {
        self.abi_minor
    }

    pub fn plugin_priorities(&self) -> &BTreeMap<String, i32> {
        &self.plugin_priorities
    }

    fn from_resolver_policy(policy: &PluginResolverPolicy) -> Self {
        Self {
            transport: policy.transport(),
            target: policy.target().to_owned(),
            architecture: policy.architecture().to_owned(),
            abi_major: policy.abi_major(),
            abi_minor: policy.abi_minor(),
            plugin_priorities: policy.plugin_priorities().clone(),
        }
    }

    fn to_resolver_policy(&self) -> Result<PluginResolverPolicy, PluginPlanError> {
        let mut policy = PluginResolverPolicy::new(
            self.transport,
            self.target.clone(),
            self.architecture.clone(),
            self.abi_major,
            self.abi_minor,
        )?;
        for (plugin_id, priority) in &self.plugin_priorities {
            policy.set_plugin_priority(plugin_id.clone(), *priority)?;
        }
        Ok(policy)
    }
}

/// One service selection stored without a live artifact owner or session.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct PluginPlanProvider {
    service: String,
    provided_version: String,
    artifact_identity: String,
}

impl PluginPlanProvider {
    pub fn service(&self) -> &str {
        &self.service
    }

    pub fn provided_version(&self) -> &str {
        &self.provided_version
    }

    pub fn artifact_identity(&self) -> &str {
        &self.artifact_identity
    }
}

/// Canonical metadata-only result that can be inspected before runtime startup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginPlan {
    schema_version: u32,
    catalog_fingerprint: String,
    catalog: Vec<PluginCatalogRecord>,
    policy: PluginPlanPolicy,
    requirements: Vec<PluginRequirement>,
    providers: Vec<PluginPlanProvider>,
    artifacts: Vec<PluginCatalogRecord>,
    fingerprint: String,
}

impl PluginPlan {
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn catalog_fingerprint(&self) -> &str {
        &self.catalog_fingerprint
    }

    pub fn catalog(&self) -> &[PluginCatalogRecord] {
        &self.catalog
    }

    pub fn policy(&self) -> &PluginPlanPolicy {
        &self.policy
    }

    pub fn requirements(&self) -> &[PluginRequirement] {
        &self.requirements
    }

    pub fn providers(&self) -> &[PluginPlanProvider] {
        &self.providers
    }

    /// Returns selected artifacts in dependency-first execution order.
    pub fn artifacts(&self) -> &[PluginCatalogRecord] {
        &self.artifacts
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// Encodes the complete plan envelope in canonical JSON field and array order.
    pub fn to_json(&self) -> Result<Vec<u8>, PluginPlanError> {
        serde_json::to_vec(self).map_err(|error| PluginPlanError::Json {
            message: error.to_string(),
        })
    }

    /// Decodes a plan and rejects stale, tampered, or noncanonical projections.
    pub fn from_json(bytes: &[u8]) -> Result<Self, PluginPlanError> {
        let wire = serde_json::from_slice::<PluginPlanWire>(bytes).map_err(|error| {
            PluginPlanError::Json {
                message: error.to_string(),
            }
        })?;
        Self::from_wire(wire)
    }

    pub(crate) fn from_resolution(
        policy: &PluginResolverPolicy,
        requirements: &[PluginRequirement],
        catalog: &PluginCatalog,
        resolution: PluginResolution,
    ) -> Result<Self, PluginPlanError> {
        let requirements = canonical_requirements(requirements)?;
        let providers = providers_from_resolution(&resolution);
        let mut plan = Self {
            schema_version: PLUGIN_PLAN_SCHEMA_VERSION,
            catalog_fingerprint: resolution.catalog_fingerprint().to_owned(),
            catalog: catalog.records().to_vec(),
            policy: PluginPlanPolicy::from_resolver_policy(policy),
            requirements,
            providers,
            artifacts: resolution.artifacts().to_vec(),
            fingerprint: String::new(),
        };
        plan.fingerprint = plan.compute_fingerprint()?;
        Ok(plan)
    }

    fn from_wire(wire: PluginPlanWire) -> Result<Self, PluginPlanError> {
        if wire.schema_version != PLUGIN_PLAN_SCHEMA_VERSION {
            return Err(PluginPlanError::UnsupportedSchemaVersion {
                expected: PLUGIN_PLAN_SCHEMA_VERSION,
                actual: wire.schema_version,
            });
        }
        if !is_lowercase_sha256(&wire.catalog_fingerprint) {
            return Err(PluginPlanError::InvalidFingerprint {
                field: "catalog_fingerprint".to_owned(),
            });
        }
        if !is_lowercase_sha256(&wire.fingerprint) {
            return Err(PluginPlanError::InvalidFingerprint {
                field: "fingerprint".to_owned(),
            });
        }

        let policy = PluginPlanPolicy {
            transport: wire.policy.transport,
            target: wire.policy.target,
            architecture: wire.policy.architecture,
            abi_major: wire.policy.abi_major,
            abi_minor: wire.policy.abi_minor,
            plugin_priorities: wire.policy.plugin_priorities,
        };
        policy.to_resolver_policy()?;

        let requirements = canonical_requirements(&wire.requirements)?;
        if requirements != wire.requirements {
            return Err(PluginPlanError::NonCanonical {
                field: "requirements".to_owned(),
            });
        }

        let providers = wire
            .providers
            .into_iter()
            .map(|provider| PluginPlanProvider {
                service: provider.service,
                provided_version: provider.provided_version,
                artifact_identity: provider.artifact_identity,
            })
            .collect::<Vec<_>>();
        validate_providers(&providers)?;
        let mut canonical_providers = providers.clone();
        canonical_providers.sort();
        if canonical_providers != providers {
            return Err(PluginPlanError::NonCanonical {
                field: "providers".to_owned(),
            });
        }

        let plan = Self {
            schema_version: wire.schema_version,
            catalog_fingerprint: wire.catalog_fingerprint,
            catalog: wire.catalog,
            policy,
            requirements,
            providers,
            artifacts: wire.artifacts,
            fingerprint: wire.fingerprint,
        };
        let canonical_catalog = PluginCatalog::from_records(plan.catalog.clone())?;
        if canonical_catalog.records() != plan.catalog {
            return Err(PluginPlanError::NonCanonical {
                field: "catalog".to_owned(),
            });
        }
        let actual_catalog_fingerprint = catalog_fingerprint_for_records(&plan.catalog)?;
        if actual_catalog_fingerprint != plan.catalog_fingerprint {
            return Err(PluginPlanError::CatalogFingerprintMismatch {
                expected: actual_catalog_fingerprint,
                actual: plan.catalog_fingerprint,
            });
        }
        plan.validate_resolution_projection()?;
        let expected = plan.compute_fingerprint()?;
        if expected != plan.fingerprint {
            return Err(PluginPlanError::FingerprintMismatch {
                expected,
                actual: plan.fingerprint,
            });
        }
        Ok(plan)
    }

    fn validate_resolution_projection(&self) -> Result<(), PluginPlanError> {
        let catalog = PluginCatalog::from_records(self.catalog.clone())?;
        let policy = self.policy.to_resolver_policy()?;
        let resolution = PluginResolver::new(&catalog, policy).resolve(&self.requirements)?;
        if providers_from_resolution(&resolution) != self.providers {
            return Err(PluginPlanError::ProjectionMismatch {
                field: "providers".to_owned(),
            });
        }
        if resolution.artifacts() != self.artifacts {
            let mut expected = resolution.artifacts().to_vec();
            let mut actual = self.artifacts.clone();
            expected.sort_by_key(PluginCatalogRecord::canonical_identity_key);
            actual.sort_by_key(PluginCatalogRecord::canonical_identity_key);
            return Err(if expected == actual {
                PluginPlanError::NonCanonical {
                    field: "artifacts".to_owned(),
                }
            } else {
                PluginPlanError::ProjectionMismatch {
                    field: "artifacts".to_owned(),
                }
            });
        }
        Ok(())
    }

    fn compute_fingerprint(&self) -> Result<String, PluginPlanError> {
        let payload = PluginPlanFingerprintPayload {
            schema_version: self.schema_version,
            catalog_fingerprint: &self.catalog_fingerprint,
            catalog: &self.catalog,
            policy: &self.policy,
            requirements: &self.requirements,
            providers: &self.providers,
            artifacts: &self.artifacts,
        };
        let bytes = serde_json::to_vec(&payload).map_err(|error| PluginPlanError::Json {
            message: error.to_string(),
        })?;
        Ok(hex::encode(Sha256::digest(bytes)))
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PluginPlanError {
    #[error("failed to decode or encode plugin plan JSON: {message}")]
    Json { message: String },
    #[error("unsupported plugin plan schema version {actual}; expected {expected}")]
    UnsupportedSchemaVersion { expected: u32, actual: u32 },
    #[error("plugin plan field `{field}` is not canonical lowercase SHA-256")]
    InvalidFingerprint { field: String },
    #[error("plugin plan field `{field}` is not in canonical order")]
    NonCanonical { field: String },
    #[error("plugin plan field `{field}` does not match the resolver projection")]
    ProjectionMismatch { field: String },
    #[error("invalid plugin plan requirements: {message}")]
    InvalidRequirements { message: String },
    #[error("plugin plan contains more than {limit} selected providers")]
    TooManyProviders { limit: usize },
    #[error("plugin plan contains duplicate provider service `{service}`")]
    DuplicateProvider { service: String },
    #[error("invalid plugin plan provider `{service}`: {message}")]
    InvalidProvider { service: String, message: String },
    #[error("plugin plan fingerprint mismatch: expected {expected}, got {actual}")]
    FingerprintMismatch { expected: String, actual: String },
    #[error("plugin plan catalog fingerprint mismatch: expected {expected}, got {actual}")]
    CatalogFingerprintMismatch { expected: String, actual: String },
    #[error(transparent)]
    Catalog(#[from] PluginCatalogError),
    #[error(transparent)]
    Resolution(#[from] PluginResolutionError),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginPlanWire {
    schema_version: u32,
    catalog_fingerprint: String,
    catalog: Vec<PluginCatalogRecord>,
    policy: PluginPlanPolicyWire,
    requirements: Vec<PluginRequirement>,
    providers: Vec<PluginPlanProviderWire>,
    artifacts: Vec<PluginCatalogRecord>,
    fingerprint: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginPlanPolicyWire {
    transport: PluginArtifactTransport,
    target: String,
    architecture: String,
    abi_major: u16,
    abi_minor: u16,
    plugin_priorities: BTreeMap<String, i32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginPlanProviderWire {
    service: String,
    provided_version: String,
    artifact_identity: String,
}

#[derive(Serialize)]
struct PluginPlanFingerprintPayload<'a> {
    schema_version: u32,
    catalog_fingerprint: &'a str,
    catalog: &'a [PluginCatalogRecord],
    policy: &'a PluginPlanPolicy,
    requirements: &'a [PluginRequirement],
    providers: &'a [PluginPlanProvider],
    artifacts: &'a [PluginCatalogRecord],
}

fn canonical_requirements(
    requirements: &[PluginRequirement],
) -> Result<Vec<PluginRequirement>, PluginPlanError> {
    validate_plugin_requirements(requirements).map_err(|error| {
        PluginPlanError::InvalidRequirements {
            message: error.to_string(),
        }
    })?;
    let mut requirements = requirements.to_vec();
    requirements.sort();
    Ok(requirements)
}

fn providers_from_resolution(resolution: &PluginResolution) -> Vec<PluginPlanProvider> {
    resolution
        .providers()
        .iter()
        .map(|provider| PluginPlanProvider {
            service: provider.service().to_owned(),
            provided_version: provider.provided_version().to_owned(),
            artifact_identity: provider.artifact().canonical_identity_key(),
        })
        .collect()
}

fn catalog_fingerprint_for_records(
    records: &[PluginCatalogRecord],
) -> Result<String, PluginPlanError> {
    Ok(PluginCatalog::from_records(records.to_vec())?
        .fingerprint()
        .to_owned())
}

fn validate_providers(providers: &[PluginPlanProvider]) -> Result<(), PluginPlanError> {
    if providers.len() > MAX_PLUGIN_PLAN_PROVIDERS {
        return Err(PluginPlanError::TooManyProviders {
            limit: MAX_PLUGIN_PLAN_PROVIDERS,
        });
    }
    let mut services = BTreeSet::new();
    for provider in providers {
        let provision = PluginProvision {
            service: provider.service.clone(),
            version: provider.provided_version.clone(),
        };
        validate_plugin_provisions(&[provision]).map_err(|error| {
            PluginPlanError::InvalidProvider {
                service: provider.service.clone(),
                message: error.to_string(),
            }
        })?;
        if !services.insert(provider.service.clone()) {
            return Err(PluginPlanError::DuplicateProvider {
                service: provider.service.clone(),
            });
        }
        if provider.artifact_identity.is_empty() {
            return Err(PluginPlanError::InvalidProvider {
                service: provider.service.clone(),
                message: "artifact identity must not be empty".to_owned(),
            });
        }
    }
    Ok(())
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
