//! Deterministic, metadata-only provider resolution.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use semver::{Version, VersionReq};
use thiserror::Error;

use crate::{
    MAX_PLUGIN_ARCHITECTURE_BYTES, MAX_PLUGIN_CATALOG_RECORDS, MAX_PLUGIN_REQUIREMENTS,
    MAX_PLUGIN_TARGET_BYTES, PluginArtifactTransport, PluginCatalog, PluginCatalogRecord,
    PluginPlan, PluginPlanError, PluginRequirement, PluginTransport, validate_plugin_requirements,
};

/// Upper bound for accumulated root and transitive requirement constraints.
pub const MAX_PLUGIN_RESOLUTION_CONSTRAINTS: usize =
    MAX_PLUGIN_CATALOG_RECORDS * MAX_PLUGIN_REQUIREMENTS;
/// Upper bound for deterministic backtracking states derived from catalog size.
pub const MAX_PLUGIN_RESOLUTION_STATES: usize = MAX_PLUGIN_CATALOG_RECORDS * 4;

/// Host-owned compatibility and preference inputs for one resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginResolverPolicy {
    transport: PluginArtifactTransport,
    target: String,
    architecture: String,
    abi_major: u16,
    abi_minor: u16,
    plugin_priorities: BTreeMap<String, i32>,
}

impl PluginResolverPolicy {
    pub fn new(
        transport: PluginArtifactTransport,
        target: impl Into<String>,
        architecture: impl Into<String>,
        abi_major: u16,
        abi_minor: u16,
    ) -> Result<Self, PluginResolutionError> {
        let target = target.into();
        let architecture = architecture.into();
        validate_policy_text("target", &target, MAX_PLUGIN_TARGET_BYTES)?;
        validate_policy_text("architecture", &architecture, MAX_PLUGIN_ARCHITECTURE_BYTES)?;
        if abi_major == 0 {
            return Err(PluginResolutionError::InvalidPolicy {
                field: "abi_major".to_owned(),
                message: "must be greater than zero".to_owned(),
            });
        }
        Ok(Self {
            transport,
            target,
            architecture,
            abi_major,
            abi_minor,
            plugin_priorities: BTreeMap::new(),
        })
    }

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

    pub fn set_plugin_priority(
        &mut self,
        plugin_id: impl Into<String>,
        priority: i32,
    ) -> Result<(), PluginResolutionError> {
        let plugin_id = plugin_id.into();
        let transport = match self.transport {
            PluginArtifactTransport::Native => PluginTransport::Native,
            PluginArtifactTransport::Wasm => PluginTransport::Wasm,
        };
        crate::PluginReference::new(plugin_id.clone(), None, transport).map_err(|error| {
            PluginResolutionError::InvalidPolicy {
                field: "plugin_priorities.plugin_id".to_owned(),
                message: error.to_string(),
            }
        })?;
        if !self.plugin_priorities.contains_key(&plugin_id)
            && self.plugin_priorities.len() >= MAX_PLUGIN_CATALOG_RECORDS
        {
            return Err(PluginResolutionError::InvalidPolicy {
                field: "plugin_priorities".to_owned(),
                message: format!("must contain at most {MAX_PLUGIN_CATALOG_RECORDS} entries"),
            });
        }
        self.plugin_priorities.insert(plugin_id, priority);
        Ok(())
    }

    pub fn plugin_priority(&self, plugin_id: &str) -> i32 {
        self.plugin_priorities
            .get(plugin_id)
            .copied()
            .unwrap_or_default()
    }

    pub fn plugin_priorities(&self) -> &BTreeMap<String, i32> {
        &self.plugin_priorities
    }

    fn accepts(&self, record: &PluginCatalogRecord) -> bool {
        let descriptor = record.descriptor();
        descriptor.transport == self.transport
            && descriptor.target == self.target
            && descriptor.architecture == self.architecture
            && descriptor.abi_major == self.abi_major
            && descriptor.abi_minor_min <= self.abi_minor
            && descriptor.abi_minor_max >= self.abi_minor
    }
}

/// One selected service provider and its immutable artifact metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginResolvedProvider {
    service: String,
    provided_version: String,
    artifact: PluginCatalogRecord,
}

impl PluginResolvedProvider {
    pub fn service(&self) -> &str {
        &self.service
    }

    pub fn provided_version(&self) -> &str {
        &self.provided_version
    }

    pub fn artifact(&self) -> &PluginCatalogRecord {
        &self.artifact
    }
}

/// Pure result of provider resolution. Artifacts are ordered dependency-first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginResolution {
    catalog_fingerprint: String,
    providers: Vec<PluginResolvedProvider>,
    artifacts: Vec<PluginCatalogRecord>,
}

impl PluginResolution {
    pub fn catalog_fingerprint(&self) -> &str {
        &self.catalog_fingerprint
    }

    pub fn providers(&self) -> &[PluginResolvedProvider] {
        &self.providers
    }

    pub fn artifacts(&self) -> &[PluginCatalogRecord] {
        &self.artifacts
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PluginResolutionError {
    #[error("invalid plugin resolver policy field `{field}`: {message}")]
    InvalidPolicy { field: String, message: String },
    #[error("invalid root plugin requirements: {message}")]
    InvalidRequirements { message: String },
    #[error("catalog invariant failed for `{artifact_identity}`: {message}")]
    InvalidCatalogRecord {
        artifact_identity: String,
        message: String,
    },
    #[error(
        "no provider for service `{service}` matches the host policy; requirements={requirements:?}, catalog_candidates={catalog_candidates}, policy_candidates={policy_candidates}"
    )]
    MissingProvider {
        service: String,
        requirements: Vec<String>,
        catalog_candidates: usize,
        policy_candidates: usize,
    },
    #[error(
        "no version of service `{service}` satisfies {requirements:?}; available versions={available_versions:?}"
    )]
    VersionConflict {
        service: String,
        requirements: Vec<String>,
        available_versions: Vec<String>,
    },
    #[error(
        "plugin `{plugin_id}` would resolve to conflicting artifacts `{selected_identity}` and `{candidate_identity}`"
    )]
    PluginIdentityConflict {
        plugin_id: String,
        selected_identity: String,
        candidate_identity: String,
    },
    #[error("plugin dependency cycle: {artifact_identities:?}")]
    DependencyCycle { artifact_identities: Vec<String> },
    #[error("plugin resolution exceeded {limit} accumulated constraints")]
    ConstraintLimitExceeded { limit: usize },
    #[error("plugin resolution exceeded {limit} deterministic search states")]
    SearchLimitExceeded { limit: usize },
}

/// Resolves service providers from an immutable catalog without artifact I/O.
pub struct PluginResolver<'a> {
    catalog: &'a PluginCatalog,
    policy: PluginResolverPolicy,
}

impl<'a> PluginResolver<'a> {
    pub fn new(catalog: &'a PluginCatalog, policy: PluginResolverPolicy) -> Self {
        Self { catalog, policy }
    }

    pub fn policy(&self) -> &PluginResolverPolicy {
        &self.policy
    }

    pub fn catalog(&self) -> &PluginCatalog {
        self.catalog
    }

    pub fn resolve(
        &self,
        root_requirements: &[PluginRequirement],
    ) -> Result<PluginResolution, PluginResolutionError> {
        validate_plugin_requirements(root_requirements).map_err(|error| {
            PluginResolutionError::InvalidRequirements {
                message: error.to_string(),
            }
        })?;

        let mut initial = ResolverState::default();
        for requirement in root_requirements {
            initial.add_constraint(requirement, RequirementOwner::Host)?;
        }
        let mut pending = vec![initial];
        let mut examined = 0_usize;
        let mut first_failure = None;

        while let Some(state) = pending.pop() {
            if examined >= MAX_PLUGIN_RESOLUTION_STATES {
                return Err(PluginResolutionError::SearchLimitExceeded {
                    limit: MAX_PLUGIN_RESOLUTION_STATES,
                });
            }
            examined += 1;
            let Some(service) = state.next_unresolved_service() else {
                match self.finish(state) {
                    Ok(resolution) => return Ok(resolution),
                    Err(error) => {
                        remember_first(&mut first_failure, error);
                        continue;
                    }
                }
            };

            let candidates = match self.candidates_for(&state, &service) {
                Ok(candidates) => candidates,
                Err(error) => {
                    remember_first(&mut first_failure, error);
                    continue;
                }
            };
            let mut next_states = Vec::with_capacity(candidates.len());
            let mut candidate_failure = None;
            for candidate in candidates {
                let mut next = state.clone();
                match self.apply_candidate(&mut next, &service, candidate) {
                    Ok(()) => next_states.push(next),
                    Err(error) => remember_first(&mut candidate_failure, error),
                }
            }
            if next_states.is_empty()
                && let Some(error) = candidate_failure
            {
                remember_first(&mut first_failure, error);
            }
            for next in next_states.into_iter().rev() {
                if pending.len() + examined >= MAX_PLUGIN_RESOLUTION_STATES {
                    return Err(PluginResolutionError::SearchLimitExceeded {
                        limit: MAX_PLUGIN_RESOLUTION_STATES,
                    });
                }
                pending.push(next);
            }
        }

        Err(
            first_failure.unwrap_or_else(|| PluginResolutionError::InvalidRequirements {
                message: "resolution ended without a result or typed provider failure".to_owned(),
            }),
        )
    }

    /// Resolves and freezes a canonical metadata-only plan.
    pub fn resolve_plan(
        &self,
        root_requirements: &[PluginRequirement],
    ) -> Result<PluginPlan, PluginPlanError> {
        let resolution = self.resolve(root_requirements)?;
        PluginPlan::from_resolution(&self.policy, root_requirements, self.catalog, resolution)
    }

    fn candidates_for(
        &self,
        state: &ResolverState,
        service: &str,
    ) -> Result<Vec<Candidate>, PluginResolutionError> {
        let constraints = state.requirements.get(service).ok_or_else(|| {
            PluginResolutionError::InvalidRequirements {
                message: format!("unresolved service `{service}` has no constraints"),
            }
        })?;
        let mut catalog_candidates = 0_usize;
        let mut policy_candidates = 0_usize;
        let mut available_versions = Vec::new();
        let mut candidates = Vec::new();

        for (record_index, record) in self.catalog.records().iter().enumerate() {
            let plugin_version = Version::parse(&record.descriptor().version).map_err(|error| {
                PluginResolutionError::InvalidCatalogRecord {
                    artifact_identity: record.canonical_identity_key(),
                    message: format!("invalid plugin version: {error}"),
                }
            })?;
            for (provision_index, provision) in record.descriptor().provides.iter().enumerate() {
                if provision.service != service {
                    continue;
                }
                catalog_candidates += 1;
                if !self.policy.accepts(record) {
                    continue;
                }
                policy_candidates += 1;
                let service_version = Version::parse(&provision.version).map_err(|error| {
                    PluginResolutionError::InvalidCatalogRecord {
                        artifact_identity: record.canonical_identity_key(),
                        message: format!("invalid provided service version: {error}"),
                    }
                })?;
                available_versions.push((service_version.clone(), provision.version.clone()));
                if constraints
                    .iter()
                    .all(|constraint| constraint.parsed.matches(&service_version))
                {
                    candidates.push(Candidate {
                        record_index,
                        provision_index,
                        service_version,
                        plugin_version: plugin_version.clone(),
                        priority: self.policy.plugin_priority(&record.descriptor().plugin_id),
                        plugin_id: record.descriptor().plugin_id.clone(),
                        artifact_identity: record.canonical_identity_key(),
                    });
                }
            }
        }

        if catalog_candidates == 0 || policy_candidates == 0 {
            return Err(PluginResolutionError::MissingProvider {
                service: service.to_owned(),
                requirements: describe_constraints(constraints),
                catalog_candidates,
                policy_candidates,
            });
        }
        if candidates.is_empty() {
            available_versions.sort();
            available_versions.dedup_by(|left, right| left.1 == right.1);
            return Err(PluginResolutionError::VersionConflict {
                service: service.to_owned(),
                requirements: describe_constraints(constraints),
                available_versions: available_versions
                    .into_iter()
                    .map(|(_, version)| version)
                    .collect(),
            });
        }
        candidates.sort_by(candidate_order);
        Ok(candidates)
    }

    fn apply_candidate(
        &self,
        state: &mut ResolverState,
        service: &str,
        candidate: Candidate,
    ) -> Result<(), PluginResolutionError> {
        let record = self
            .catalog
            .records()
            .get(candidate.record_index)
            .ok_or_else(|| PluginResolutionError::InvalidCatalogRecord {
                artifact_identity: candidate.artifact_identity.clone(),
                message: "candidate record index is outside the catalog".to_owned(),
            })?;
        let artifact_identity = record.canonical_identity_key();
        if let Some(selected_identity) = state.plugin_artifacts.get(&record.descriptor().plugin_id)
            && selected_identity != &artifact_identity
        {
            return Err(PluginResolutionError::PluginIdentityConflict {
                plugin_id: record.descriptor().plugin_id.clone(),
                selected_identity: selected_identity.clone(),
                candidate_identity: artifact_identity,
            });
        }

        state.selected_services.insert(
            service.to_owned(),
            SelectedProvider {
                record_index: candidate.record_index,
                provision_index: candidate.provision_index,
            },
        );
        if state
            .selected_artifacts
            .insert(artifact_identity.clone(), candidate.record_index)
            .is_none()
        {
            state.plugin_artifacts.insert(
                record.descriptor().plugin_id.clone(),
                artifact_identity.clone(),
            );
            for requirement in &record.descriptor().requires {
                state.add_constraint(
                    requirement,
                    RequirementOwner::Artifact(artifact_identity.clone()),
                )?;
                if let Some(selected) = state.selected_services.get(&requirement.service) {
                    self.validate_selected_service(state, &requirement.service, *selected)?;
                }
            }
        }
        Ok(())
    }

    fn validate_selected_service(
        &self,
        state: &ResolverState,
        service: &str,
        selected: SelectedProvider,
    ) -> Result<(), PluginResolutionError> {
        let record = self
            .catalog
            .records()
            .get(selected.record_index)
            .ok_or_else(|| PluginResolutionError::InvalidCatalogRecord {
                artifact_identity: format!("record-index:{}", selected.record_index),
                message: "selected provider index is outside the catalog".to_owned(),
            })?;
        let provision = record
            .descriptor()
            .provides
            .get(selected.provision_index)
            .ok_or_else(|| PluginResolutionError::InvalidCatalogRecord {
                artifact_identity: record.canonical_identity_key(),
                message: "selected provision index is outside the descriptor".to_owned(),
            })?;
        let version = Version::parse(&provision.version).map_err(|error| {
            PluginResolutionError::InvalidCatalogRecord {
                artifact_identity: record.canonical_identity_key(),
                message: format!("invalid selected service version: {error}"),
            }
        })?;
        let constraints = state.requirements.get(service).ok_or_else(|| {
            PluginResolutionError::InvalidRequirements {
                message: format!("selected service `{service}` has no constraints"),
            }
        })?;
        if constraints
            .iter()
            .all(|constraint| constraint.parsed.matches(&version))
        {
            return Ok(());
        }
        Err(PluginResolutionError::VersionConflict {
            service: service.to_owned(),
            requirements: describe_constraints(constraints),
            available_versions: vec![provision.version.clone()],
        })
    }

    fn finish(&self, state: ResolverState) -> Result<PluginResolution, PluginResolutionError> {
        let artifact_order = self.topological_order(&state)?;
        let providers = state
            .selected_services
            .iter()
            .map(|(service, selected)| {
                let record = self
                    .catalog
                    .records()
                    .get(selected.record_index)
                    .ok_or_else(|| PluginResolutionError::InvalidCatalogRecord {
                        artifact_identity: format!("record-index:{}", selected.record_index),
                        message: "selected provider index is outside the catalog".to_owned(),
                    })?;
                let provision = record
                    .descriptor()
                    .provides
                    .get(selected.provision_index)
                    .ok_or_else(|| PluginResolutionError::InvalidCatalogRecord {
                        artifact_identity: record.canonical_identity_key(),
                        message: "selected provision index is outside the descriptor".to_owned(),
                    })?;
                Ok(PluginResolvedProvider {
                    service: service.clone(),
                    provided_version: provision.version.clone(),
                    artifact: record.clone(),
                })
            })
            .collect::<Result<Vec<_>, PluginResolutionError>>()?;
        let artifacts = artifact_order
            .iter()
            .map(|identity| {
                let index = state.selected_artifacts.get(identity).ok_or_else(|| {
                    PluginResolutionError::InvalidCatalogRecord {
                        artifact_identity: identity.clone(),
                        message: "topological artifact is absent from selection".to_owned(),
                    }
                })?;
                self.catalog.records().get(*index).cloned().ok_or_else(|| {
                    PluginResolutionError::InvalidCatalogRecord {
                        artifact_identity: identity.clone(),
                        message: "topological artifact index is outside the catalog".to_owned(),
                    }
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(PluginResolution {
            catalog_fingerprint: self.catalog.fingerprint().to_owned(),
            providers,
            artifacts,
        })
    }

    fn topological_order(
        &self,
        state: &ResolverState,
    ) -> Result<Vec<String>, PluginResolutionError> {
        let mut adjacency = state
            .selected_artifacts
            .keys()
            .map(|identity| (identity.clone(), BTreeSet::new()))
            .collect::<BTreeMap<_, _>>();
        let mut indegree = state
            .selected_artifacts
            .keys()
            .map(|identity| (identity.clone(), 0_usize))
            .collect::<BTreeMap<_, _>>();
        let mut order_keys = BTreeMap::new();
        for (identity, record_index) in &state.selected_artifacts {
            let record = self.catalog.records().get(*record_index).ok_or_else(|| {
                PluginResolutionError::InvalidCatalogRecord {
                    artifact_identity: identity.clone(),
                    message: "selected artifact index is outside the catalog".to_owned(),
                }
            })?;
            order_keys.insert(identity.clone(), ArtifactOrderKey::from_record(record)?);
        }

        for (dependent_identity, record_index) in &state.selected_artifacts {
            let record = self.catalog.records().get(*record_index).ok_or_else(|| {
                PluginResolutionError::InvalidCatalogRecord {
                    artifact_identity: dependent_identity.clone(),
                    message: "selected artifact index is outside the catalog".to_owned(),
                }
            })?;
            for requirement in &record.descriptor().requires {
                let provider = state
                    .selected_services
                    .get(&requirement.service)
                    .ok_or_else(|| PluginResolutionError::MissingProvider {
                        service: requirement.service.clone(),
                        requirements: vec![requirement.requirement.clone()],
                        catalog_candidates: 0,
                        policy_candidates: 0,
                    })?;
                let provider_record = self
                    .catalog
                    .records()
                    .get(provider.record_index)
                    .ok_or_else(|| PluginResolutionError::InvalidCatalogRecord {
                        artifact_identity: format!("record-index:{}", provider.record_index),
                        message: "dependency provider index is outside the catalog".to_owned(),
                    })?;
                let provider_identity = provider_record.canonical_identity_key();
                let inserted = adjacency
                    .entry(provider_identity.clone())
                    .or_default()
                    .insert(dependent_identity.clone());
                if inserted {
                    let value = indegree.get_mut(dependent_identity).ok_or_else(|| {
                        PluginResolutionError::InvalidCatalogRecord {
                            artifact_identity: dependent_identity.clone(),
                            message: "dependency target is absent from selection".to_owned(),
                        }
                    })?;
                    *value = value.saturating_add(1);
                }
            }
        }

        let mut ready = indegree
            .iter()
            .filter_map(|(identity, count)| {
                (*count == 0)
                    .then(|| order_keys.get(identity).cloned())
                    .flatten()
            })
            .collect::<BTreeSet<_>>();
        let mut order = Vec::with_capacity(indegree.len());
        while let Some(key) = ready.pop_first() {
            let identity = key.identity;
            order.push(identity.clone());
            if let Some(dependents) = adjacency.get(&identity) {
                for dependent in dependents {
                    let count = indegree.get_mut(dependent).ok_or_else(|| {
                        PluginResolutionError::InvalidCatalogRecord {
                            artifact_identity: dependent.clone(),
                            message: "dependency target is absent from indegree map".to_owned(),
                        }
                    })?;
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        let key = order_keys.get(dependent).cloned().ok_or_else(|| {
                            PluginResolutionError::InvalidCatalogRecord {
                                artifact_identity: dependent.clone(),
                                message: "dependency target has no stable order key".to_owned(),
                            }
                        })?;
                        ready.insert(key);
                    }
                }
            }
        }
        if order.len() == indegree.len() {
            return Ok(order);
        }
        let adjacency = adjacency
            .into_iter()
            .map(|(identity, dependents)| {
                let mut dependents = dependents.into_iter().collect::<Vec<_>>();
                dependents.sort_by(|left, right| {
                    order_keys
                        .get(left)
                        .cmp(&order_keys.get(right))
                        .then_with(|| left.cmp(right))
                });
                (identity, dependents)
            })
            .collect::<BTreeMap<String, Vec<String>>>();
        Err(PluginResolutionError::DependencyCycle {
            artifact_identities: find_cycle(&adjacency, &order_keys),
        })
    }
}

#[derive(Debug, Clone)]
struct RequirementConstraint {
    requirement: String,
    parsed: VersionReq,
    owner: RequirementOwner,
}

#[derive(Debug, Clone)]
enum RequirementOwner {
    Host,
    Artifact(String),
}

impl RequirementOwner {
    fn label(&self) -> &str {
        match self {
            Self::Host => "host",
            Self::Artifact(identity) => identity,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct SelectedProvider {
    record_index: usize,
    provision_index: usize,
}

#[derive(Debug, Clone, Default)]
struct ResolverState {
    requirements: BTreeMap<String, Vec<RequirementConstraint>>,
    selected_services: BTreeMap<String, SelectedProvider>,
    selected_artifacts: BTreeMap<String, usize>,
    plugin_artifacts: BTreeMap<String, String>,
    constraint_count: usize,
}

impl ResolverState {
    fn add_constraint(
        &mut self,
        requirement: &PluginRequirement,
        owner: RequirementOwner,
    ) -> Result<(), PluginResolutionError> {
        if self.constraint_count >= MAX_PLUGIN_RESOLUTION_CONSTRAINTS {
            return Err(PluginResolutionError::ConstraintLimitExceeded {
                limit: MAX_PLUGIN_RESOLUTION_CONSTRAINTS,
            });
        }
        let parsed = VersionReq::parse(&requirement.requirement).map_err(|error| {
            PluginResolutionError::InvalidRequirements {
                message: format!(
                    "service `{}` has invalid semver requirement: {error}",
                    requirement.service
                ),
            }
        })?;
        self.requirements
            .entry(requirement.service.clone())
            .or_default()
            .push(RequirementConstraint {
                requirement: requirement.requirement.clone(),
                parsed,
                owner,
            });
        self.constraint_count += 1;
        Ok(())
    }

    fn next_unresolved_service(&self) -> Option<String> {
        self.requirements
            .keys()
            .find(|service| !self.selected_services.contains_key(*service))
            .cloned()
    }
}

#[derive(Debug)]
struct Candidate {
    record_index: usize,
    provision_index: usize,
    service_version: Version,
    plugin_version: Version,
    priority: i32,
    plugin_id: String,
    artifact_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ArtifactOrderKey {
    plugin_id: String,
    plugin_version: Version,
    transport: PluginArtifactTransport,
    target: String,
    architecture: String,
    format: crate::PluginArtifactFormat,
    identity: String,
}

impl ArtifactOrderKey {
    fn from_record(record: &PluginCatalogRecord) -> Result<Self, PluginResolutionError> {
        let descriptor = record.descriptor();
        let plugin_version = Version::parse(&descriptor.version).map_err(|error| {
            PluginResolutionError::InvalidCatalogRecord {
                artifact_identity: record.canonical_identity_key(),
                message: format!("invalid plugin version: {error}"),
            }
        })?;
        Ok(Self {
            plugin_id: descriptor.plugin_id.clone(),
            plugin_version,
            transport: descriptor.transport,
            target: descriptor.target.clone(),
            architecture: descriptor.architecture.clone(),
            format: descriptor.format,
            identity: record.canonical_identity_key(),
        })
    }
}

fn candidate_order(left: &Candidate, right: &Candidate) -> Ordering {
    right
        .service_version
        .cmp(&left.service_version)
        .then_with(|| right.priority.cmp(&left.priority))
        .then_with(|| right.plugin_version.cmp(&left.plugin_version))
        .then_with(|| left.plugin_id.cmp(&right.plugin_id))
        .then_with(|| left.artifact_identity.cmp(&right.artifact_identity))
}

fn describe_constraints(constraints: &[RequirementConstraint]) -> Vec<String> {
    constraints
        .iter()
        .map(|constraint| {
            format!(
                "{} required by {}",
                constraint.requirement,
                constraint.owner.label()
            )
        })
        .collect()
}

fn remember_first(target: &mut Option<PluginResolutionError>, error: PluginResolutionError) {
    if target.is_none() {
        *target = Some(error);
    }
}

fn validate_policy_text(
    field: &str,
    value: &str,
    maximum_bytes: usize,
) -> Result<(), PluginResolutionError> {
    if value.is_empty() || value.len() > maximum_bytes {
        return Err(PluginResolutionError::InvalidPolicy {
            field: field.to_owned(),
            message: format!("must contain 1 to {maximum_bytes} UTF-8 bytes"),
        });
    }
    Ok(())
}

fn find_cycle(
    adjacency: &BTreeMap<String, Vec<String>>,
    order_keys: &BTreeMap<String, ArtifactOrderKey>,
) -> Vec<String> {
    let mut colors = adjacency
        .keys()
        .map(|identity| (identity.clone(), 0_u8))
        .collect::<BTreeMap<_, _>>();
    let starts = order_keys.values().cloned().collect::<BTreeSet<_>>();
    for start in starts.iter().map(|key| &key.identity) {
        if colors.get(start).copied().unwrap_or_default() != 0 {
            continue;
        }
        colors.insert(start.clone(), 1);
        let mut path = vec![start.clone()];
        let mut stack = vec![(start.clone(), 0_usize)];
        while let Some((identity, next_index)) = stack.last_mut() {
            let neighbors = adjacency
                .get(identity)
                .map(Vec::as_slice)
                .unwrap_or_default();
            if *next_index >= neighbors.len() {
                colors.insert(identity.clone(), 2);
                stack.pop();
                path.pop();
                continue;
            }
            let neighbor = neighbors[*next_index].clone();
            *next_index += 1;
            match colors.get(&neighbor).copied().unwrap_or_default() {
                0 => {
                    colors.insert(neighbor.clone(), 1);
                    path.push(neighbor.clone());
                    stack.push((neighbor, 0));
                }
                1 => {
                    if let Some(start_index) = path.iter().position(|node| node == &neighbor) {
                        let mut cycle = path[start_index..].to_vec();
                        cycle.push(neighbor);
                        return cycle;
                    }
                }
                _ => {}
            }
        }
    }
    adjacency.keys().cloned().collect()
}
