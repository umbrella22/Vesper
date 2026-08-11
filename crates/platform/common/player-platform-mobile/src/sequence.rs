use std::time::{Duration, Instant};

use player_runtime::{
    SequenceActivationEpoch, SequenceCacheIdentity, SequenceClockSnapshot, SequenceConfig,
    SequenceContentIdentity, SequenceCoordinator, SequenceDirection, SequenceError,
    SequenceErrorCode, SequenceEvent, SequenceEventKind, SequenceItem, SequenceItemId,
    SequenceItemsResponse, SequenceMediaKind, SequenceMode, SequenceNavigationOutcome,
    SequencePendingRequest, SequencePreloadIntent, SequenceRequestFailure, SequenceRequestId,
    SequenceRequestKind, SequenceResolvedSource, SequenceSessionGeneration, SequenceSnapshot,
    SequenceSourceReference, SequenceSourceResolutionReason, SequenceSourceRevision,
    SequenceSourceState, SequenceWarmupGoal, SequenceWarmupReport, SequenceWarmupStatus,
    SequenceWarmupTaskId, SequenceWarmupTaskSnapshot,
};
use serde::Deserialize;
use serde_json::{Value, json};

/// Maximum encoded request accepted by the mobile sequence bridge.
pub const MAX_MOBILE_SEQUENCE_JSON_BYTES: usize = 512 * 1024;
/// Maximum number of items or events accepted in one mobile sequence batch.
pub const MAX_MOBILE_SEQUENCE_BATCH_ITEMS: usize = 512;

/// Stable bridge error returned before or while executing a sequence command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobileSequenceBridgeError {
    /// Stable snake-case error code.
    pub code: &'static str,
    /// Non-sensitive diagnostic message.
    pub message: String,
}

impl MobileSequenceBridgeError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Encodes the error using the shared mobile envelope.
    pub fn to_json(&self) -> String {
        json!({
            "ok": false,
            "error": {
                "code": self.code,
                "message": self.message,
            }
        })
        .to_string()
    }
}

impl From<SequenceError> for MobileSequenceBridgeError {
    fn from(error: SequenceError) -> Self {
        Self::new(sequence_error_code(error.code), error.message)
    }
}

/// Shared Android/iOS bridge session around the authoritative Rust coordinator.
#[derive(Debug)]
pub struct MobileSequenceBridgeSession {
    coordinator: SequenceCoordinator,
}

impl MobileSequenceBridgeSession {
    /// Creates a bridge session from a bounded JSON configuration.
    pub fn from_config_json(config_json: &str) -> Result<Self, MobileSequenceBridgeError> {
        ensure_json_size(config_json)?;
        let wire: SequenceConfigWire = serde_json::from_str(config_json).map_err(|error| {
            MobileSequenceBridgeError::new(
                "invalid_json",
                format!("sequence config JSON was invalid: {error}"),
            )
        })?;
        let sequence_id = wire.sequence_id.clone();
        let config = wire.try_into_config()?;
        let coordinator = SequenceCoordinator::new(sequence_id, config)
            .map_err(MobileSequenceBridgeError::from)?;
        Ok(Self { coordinator })
    }

    /// Executes one bounded command and returns a typed JSON envelope.
    pub fn execute_json(&mut self, command_json: &str, wall_epoch_ms: u64) -> String {
        match self.try_execute_json(command_json, wall_epoch_ms) {
            Ok(value) => ok_envelope(value),
            Err(error) => error.to_json(),
        }
    }

    /// Returns the authoritative snapshot as a typed JSON envelope.
    pub fn snapshot_json(&self) -> String {
        ok_envelope(snapshot_value(&self.coordinator.snapshot(), Instant::now()))
    }

    /// Drains a bounded event batch as a typed JSON envelope.
    pub fn drain_events_json(&mut self, max_count: usize) -> String {
        if max_count == 0 || max_count > MAX_MOBILE_SEQUENCE_BATCH_ITEMS {
            return MobileSequenceBridgeError::new(
                "invalid_argument",
                "event batch size must be between 1 and 512",
            )
            .to_json();
        }
        let now = Instant::now();
        let events = self
            .coordinator
            .drain_events_bounded(max_count)
            .iter()
            .map(|event| event_value(event, now))
            .collect::<Vec<_>>();
        ok_envelope(json!({ "events": events }))
    }

    /// Returns safe preload intents for host-registry execution.
    pub fn preload_intents_json(&self, wall_epoch_ms: u64) -> String {
        let intents = self
            .coordinator
            .preload_intents(wall_epoch_ms)
            .iter()
            .map(preload_intent_value)
            .collect::<Vec<_>>();
        ok_envelope(json!({ "intents": intents }))
    }

    fn try_execute_json(
        &mut self,
        command_json: &str,
        wall_epoch_ms: u64,
    ) -> Result<Value, MobileSequenceBridgeError> {
        ensure_json_size(command_json)?;
        let command: SequenceCommandWire = serde_json::from_str(command_json).map_err(|error| {
            MobileSequenceBridgeError::new(
                "invalid_json",
                format!("sequence command JSON was invalid: {error}"),
            )
        })?;
        let now = SequenceClockSnapshot {
            wall_epoch_ms,
            monotonic: Instant::now(),
        };
        match command {
            SequenceCommandWire::Replace {
                items,
                active_item_id,
            } => {
                ensure_batch_len(items.len())?;
                let items = items
                    .into_iter()
                    .map(SequenceItemWire::try_into_item)
                    .collect::<Result<Vec<_>, _>>()?;
                self.coordinator
                    .replace(items, active_item_id.map(SequenceItemId::new), now)?;
                Ok(json!({ "applied": true }))
            }
            SequenceCommandWire::Append {
                session_generation,
                request_id,
                anchor_item_id,
                items,
                end_reached,
            } => {
                ensure_batch_len(items.len())?;
                let response = items_response(
                    session_generation,
                    request_id,
                    anchor_item_id,
                    items,
                    end_reached,
                )?;
                let accepted = self.coordinator.append(response, now)?;
                Ok(json!({ "acceptedCount": accepted }))
            }
            SequenceCommandWire::Prepend {
                session_generation,
                request_id,
                anchor_item_id,
                items,
                end_reached,
            } => {
                ensure_batch_len(items.len())?;
                let response = items_response(
                    session_generation,
                    request_id,
                    anchor_item_id,
                    items,
                    end_reached,
                )?;
                let accepted = self.coordinator.prepend(response, now)?;
                Ok(json!({ "acceptedCount": accepted }))
            }
            SequenceCommandWire::Remove { item_id } => {
                let removed = self
                    .coordinator
                    .remove(&SequenceItemId::new(item_id), now)?;
                Ok(json!({ "removed": removed }))
            }
            SequenceCommandWire::SetActive { item_id } => navigation_value(
                self.coordinator
                    .set_active(&SequenceItemId::new(item_id), now)?,
            ),
            SequenceCommandWire::Next => navigation_value(self.coordinator.next(now)?),
            SequenceCommandWire::Previous => navigation_value(self.coordinator.previous(now)?),
            SequenceCommandWire::SubmitResolvedSource { source } => {
                self.coordinator
                    .submit_resolved_source(source.try_into()?)?;
                Ok(json!({ "applied": true }))
            }
            SequenceCommandWire::MarkSourceExpired {
                item_id,
                source_revision,
            } => {
                let request_id = self.coordinator.mark_source_expired(
                    &SequenceItemId::new(item_id),
                    SequenceSourceRevision::new(source_revision),
                    now,
                )?;
                Ok(json!({ "requestId": request_id.get() }))
            }
            SequenceCommandWire::FailRequest {
                session_generation,
                request_id,
                reason_code,
            } => {
                self.coordinator.fail_request(
                    SequenceSessionGeneration::new(session_generation),
                    SequenceRequestId::new(request_id),
                    reason_code,
                )?;
                Ok(json!({ "applied": true }))
            }
            SequenceCommandWire::Tick => {
                self.coordinator.tick(now)?;
                Ok(json!({ "applied": true }))
            }
            SequenceCommandWire::ResyncPendingRequests => {
                self.coordinator.resync_pending_requests();
                Ok(json!({ "applied": true }))
            }
            SequenceCommandWire::ValidateActivationCallback {
                item_id,
                activation_epoch,
                source_revision,
            } => {
                self.coordinator.validate_activation_callback(
                    &SequenceItemId::new(item_id),
                    SequenceActivationEpoch::new(activation_epoch),
                    SequenceSourceRevision::new(source_revision),
                )?;
                Ok(json!({ "accepted": true }))
            }
            SequenceCommandWire::ReportWarmup {
                session_generation,
                task_id,
                item_id,
                source_revision,
                warmup_goal,
                status,
                expected_bytes,
                actual_bytes,
                cache_hit,
                cache_entries,
                cache_bytes,
                evicted_entries,
                reason_code,
            } => {
                let warmup_goal = warmup_goal_from_wire(&warmup_goal)?;
                let status = warmup_status_from_wire(&status)?;
                self.coordinator.report_warmup(SequenceWarmupReport {
                    session_generation: SequenceSessionGeneration::new(session_generation),
                    task_id: SequenceWarmupTaskId::new(task_id),
                    item_id: SequenceItemId::new(item_id),
                    source_revision: SequenceSourceRevision::new(source_revision),
                    warmup_goal,
                    status,
                    expected_bytes,
                    actual_bytes,
                    cache_hit,
                    cache_entries,
                    cache_bytes,
                    evicted_entries,
                    reason_code,
                })?;
                Ok(json!({ "accepted": true }))
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SequenceConfigWire {
    sequence_id: String,
    #[serde(default = "default_sequence_mode")]
    mode: String,
    #[serde(default = "default_history_limit")]
    history_limit: usize,
    #[serde(default = "default_forward_window")]
    forward_window: usize,
    #[serde(default = "default_refill_threshold")]
    refill_threshold: usize,
    #[serde(default = "default_max_items")]
    max_items: usize,
    #[serde(default = "default_max_pending_requests")]
    max_pending_requests: usize,
    #[serde(default = "default_max_events")]
    max_events: usize,
    #[serde(default = "default_request_timeout_ms")]
    request_timeout_ms: u64,
    #[serde(default = "default_source_expiry_lead_ms")]
    source_expiry_lead_ms: u64,
}

impl SequenceConfigWire {
    fn try_into_config(self) -> Result<SequenceConfig, MobileSequenceBridgeError> {
        let mode = match self.mode.as_str() {
            "finite" => SequenceMode::Finite,
            "replenishable" => SequenceMode::Replenishable,
            _ => {
                return Err(MobileSequenceBridgeError::new(
                    "unknown_enum",
                    "sequence mode must be finite or replenishable",
                ));
            }
        };
        if self.max_items > MAX_MOBILE_SEQUENCE_BATCH_ITEMS
            || self.max_pending_requests > MAX_MOBILE_SEQUENCE_BATCH_ITEMS
            || self.max_events > MAX_MOBILE_SEQUENCE_BATCH_ITEMS * 2
        {
            return Err(MobileSequenceBridgeError::new(
                "capacity_exceeded",
                "sequence wire capacities exceed mobile bridge limits",
            ));
        }
        Ok(SequenceConfig {
            mode,
            history_limit: self.history_limit,
            forward_window: self.forward_window,
            refill_threshold: self.refill_threshold,
            max_items: self.max_items,
            max_pending_requests: self.max_pending_requests,
            max_events: self.max_events,
            request_timeout: Duration::from_millis(self.request_timeout_ms),
            source_expiry_lead: Duration::from_millis(self.source_expiry_lead_ms),
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum SequenceCommandWire {
    Replace {
        items: Vec<SequenceItemWire>,
        #[serde(default)]
        active_item_id: Option<String>,
    },
    Append {
        session_generation: u64,
        request_id: u64,
        #[serde(default)]
        anchor_item_id: Option<String>,
        items: Vec<SequenceItemWire>,
        end_reached: bool,
    },
    Prepend {
        session_generation: u64,
        request_id: u64,
        #[serde(default)]
        anchor_item_id: Option<String>,
        items: Vec<SequenceItemWire>,
        end_reached: bool,
    },
    Remove {
        item_id: String,
    },
    SetActive {
        item_id: String,
    },
    Next,
    Previous,
    SubmitResolvedSource {
        source: ResolvedSourceWire,
    },
    MarkSourceExpired {
        item_id: String,
        source_revision: u64,
    },
    FailRequest {
        session_generation: u64,
        request_id: u64,
        reason_code: String,
    },
    Tick,
    ResyncPendingRequests,
    ValidateActivationCallback {
        item_id: String,
        activation_epoch: u64,
        source_revision: u64,
    },
    ReportWarmup {
        session_generation: u64,
        task_id: u64,
        item_id: String,
        source_revision: u64,
        warmup_goal: String,
        status: String,
        expected_bytes: u64,
        actual_bytes: u64,
        #[serde(default)]
        cache_hit: Option<bool>,
        #[serde(default)]
        cache_entries: u32,
        #[serde(default)]
        cache_bytes: u64,
        #[serde(default)]
        evicted_entries: u64,
        #[serde(default)]
        reason_code: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SequenceItemWire {
    item_id: String,
    provider_namespace: String,
    content_identity: String,
    media_kind: String,
    #[serde(default)]
    provider_metadata_ref: Option<String>,
    #[serde(default)]
    preload_profile: SequencePreloadProfileWire,
    #[serde(default)]
    resolved_source: Option<InitialResolvedSourceWire>,
}

impl SequenceItemWire {
    fn try_into_item(self) -> Result<SequenceItem, MobileSequenceBridgeError> {
        let media_kind = media_kind_from_wire(&self.media_kind)?;
        let content = SequenceContentIdentity::new(self.provider_namespace, self.content_identity);
        let mut item = if let Some(source) = self.resolved_source {
            let cache_identity = source.cache_identity.try_into()?;
            SequenceItem::resolved(
                self.item_id,
                content,
                media_kind,
                source.source_reference,
                cache_identity,
                source.expires_at_epoch_ms,
            )
        } else {
            SequenceItem::unresolved(self.item_id, content, media_kind)
        };
        item.provider_metadata_ref = normalize_optional_reference(self.provider_metadata_ref)?;
        item.preload_profile = self.preload_profile.into();
        Ok(item)
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SequencePreloadProfileWire {
    #[serde(default)]
    expected_memory_bytes: u64,
    #[serde(default)]
    expected_disk_bytes: u64,
    #[serde(default)]
    ttl_ms: Option<u64>,
    #[serde(default)]
    warmup_window_ms: Option<u64>,
}

impl From<SequencePreloadProfileWire> for player_runtime::SequencePreloadProfile {
    fn from(value: SequencePreloadProfileWire) -> Self {
        Self {
            expected_memory_bytes: value.expected_memory_bytes,
            expected_disk_bytes: value.expected_disk_bytes,
            ttl: value.ttl_ms.map(Duration::from_millis),
            warmup_window: value.warmup_window_ms.map(Duration::from_millis),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InitialResolvedSourceWire {
    source_reference: String,
    cache_identity: CacheIdentityWire,
    #[serde(default)]
    expires_at_epoch_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResolvedSourceWire {
    session_generation: u64,
    request_id: u64,
    resolution_attempt_id: u64,
    item_id: String,
    expected_source_revision: u64,
    source_revision: u64,
    source_reference: String,
    cache_identity: CacheIdentityWire,
    #[serde(default)]
    expires_at_epoch_ms: Option<u64>,
}

impl TryFrom<ResolvedSourceWire> for SequenceResolvedSource {
    type Error = MobileSequenceBridgeError;

    fn try_from(value: ResolvedSourceWire) -> Result<Self, Self::Error> {
        Ok(Self {
            session_generation: SequenceSessionGeneration::new(value.session_generation),
            request_id: SequenceRequestId::new(value.request_id),
            attempt_id: player_runtime::SequenceResolutionAttemptId::new(
                value.resolution_attempt_id,
            ),
            item_id: SequenceItemId::new(value.item_id),
            expected_revision: SequenceSourceRevision::new(value.expected_source_revision),
            source_revision: SequenceSourceRevision::new(value.source_revision),
            source_reference: SequenceSourceReference::new(value.source_reference),
            cache_identity: value.cache_identity.try_into()?,
            expires_at_epoch_ms: value.expires_at_epoch_ms,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CacheIdentityWire {
    provider_namespace: String,
    content_identity: String,
    rendition_identity: String,
    resource_identity: String,
    access_partition: String,
    source_revision: u64,
}

impl TryFrom<CacheIdentityWire> for SequenceCacheIdentity {
    type Error = MobileSequenceBridgeError;

    fn try_from(value: CacheIdentityWire) -> Result<Self, Self::Error> {
        for field in [
            value.provider_namespace.as_str(),
            value.content_identity.as_str(),
            value.rendition_identity.as_str(),
            value.resource_identity.as_str(),
            value.access_partition.as_str(),
        ] {
            if field.len() > 512 || field.chars().any(char::is_control) || field.contains("://") {
                return Err(MobileSequenceBridgeError::new(
                    "invalid_argument",
                    "cache identity fields must be bounded stable identities",
                ));
            }
        }
        Ok(SequenceCacheIdentity::new(
            value.provider_namespace,
            value.content_identity,
            value.rendition_identity,
            value.resource_identity,
            value.access_partition,
            SequenceSourceRevision::new(value.source_revision),
        ))
    }
}

fn items_response(
    session_generation: u64,
    request_id: u64,
    anchor_item_id: Option<String>,
    items: Vec<SequenceItemWire>,
    end_reached: bool,
) -> Result<SequenceItemsResponse, MobileSequenceBridgeError> {
    Ok(SequenceItemsResponse {
        session_generation: SequenceSessionGeneration::new(session_generation),
        request_id: SequenceRequestId::new(request_id),
        anchor_item_id: anchor_item_id.map(SequenceItemId::new),
        items: items
            .into_iter()
            .map(SequenceItemWire::try_into_item)
            .collect::<Result<Vec<_>, _>>()?,
        end_reached,
    })
}

fn snapshot_value(snapshot: &SequenceSnapshot, now: Instant) -> Value {
    json!({
        "sequenceId": snapshot.sequence_id.as_str(),
        "sessionGeneration": snapshot.session_generation.get(),
        "activationEpoch": snapshot.activation_epoch.get(),
        "items": snapshot.items.iter().map(|item| json!({
            "item": item_value(&item.item),
            "index": item.index,
            "isActive": item.is_active,
        })).collect::<Vec<_>>(),
        "activeItemId": snapshot.active_item_id.as_ref().map(SequenceItemId::as_str),
        "pendingRequests": snapshot.pending_requests.iter().map(|request| pending_request_value(request, now)).collect::<Vec<_>>(),
        "requestFailures": snapshot.request_failures.iter().map(request_failure_value).collect::<Vec<_>>(),
        "previousEndReached": snapshot.previous_end_reached,
        "nextEndReached": snapshot.next_end_reached,
        "droppedEvents": snapshot.dropped_events,
        "warmupTasks": snapshot.warmup_tasks.iter().map(warmup_task_value).collect::<Vec<_>>(),
        "warmupStats": {
            "started": snapshot.warmup_stats.started,
            "completed": snapshot.warmup_stats.completed,
            "cancelled": snapshot.warmup_stats.cancelled,
            "failed": snapshot.warmup_stats.failed,
            "unsupported": snapshot.warmup_stats.unsupported,
            "cacheHits": snapshot.warmup_stats.cache_hits,
            "cacheMisses": snapshot.warmup_stats.cache_misses,
            "expectedBytes": snapshot.warmup_stats.expected_bytes,
            "actualBytes": snapshot.warmup_stats.actual_bytes,
            "evictedEntries": snapshot.warmup_stats.evicted_entries,
        },
    })
}

fn item_value(item: &SequenceItem) -> Value {
    json!({
        "itemId": item.item_id.as_str(),
        "contentIdentity": {
            "providerNamespace": item.content_identity.provider_namespace,
            "value": item.content_identity.value,
        },
        "mediaKind": media_kind_wire(item.media_kind),
        "sourceState": source_state_value(&item.source_state),
        "providerMetadataRef": item.provider_metadata_ref,
        "preloadProfile": {
            "expectedMemoryBytes": item.preload_profile.expected_memory_bytes,
            "expectedDiskBytes": item.preload_profile.expected_disk_bytes,
            "ttlMs": item.preload_profile.ttl.map(duration_millis),
            "warmupWindowMs": item.preload_profile.warmup_window.map(duration_millis),
        },
    })
}

fn source_state_value(state: &SequenceSourceState) -> Value {
    match state {
        SequenceSourceState::Unresolved => json!({ "state": "unresolved", "sourceRevision": 0 }),
        SequenceSourceState::Resolving {
            request_id,
            attempt_id,
            expected_revision,
        } => json!({
            "state": "resolving",
            "requestId": request_id.get(),
            "resolutionAttemptId": attempt_id.get(),
            "expectedSourceRevision": expected_revision.get(),
        }),
        SequenceSourceState::Resolved {
            source_reference,
            revision,
            cache_identity,
            expires_at_epoch_ms,
        } => json!({
            "state": "resolved",
            "sourceReference": source_reference.as_str(),
            "sourceRevision": revision.get(),
            "cacheIdentity": cache_identity_value(cache_identity),
            "expiresAtEpochMs": expires_at_epoch_ms,
        }),
        SequenceSourceState::Expired { revision } => json!({
            "state": "expired",
            "sourceRevision": revision.get(),
        }),
        SequenceSourceState::Failed {
            revision,
            reason_code,
        } => json!({
            "state": "failed",
            "sourceRevision": revision.get(),
            "reasonCode": reason_code,
        }),
    }
}

fn pending_request_value(request: &SequencePendingRequest, now: Instant) -> Value {
    json!({
        "request": request_kind_value(&request.kind, now),
        "deadlineRemainingMs": duration_millis(request.deadline.saturating_duration_since(now)),
        "deliveryState": match request.delivery_state {
            player_runtime::SequenceRequestDeliveryState::Pending => "pending",
            player_runtime::SequenceRequestDeliveryState::Delivered => "delivered",
        },
        "deliveryCount": request.delivery_count,
    })
}

fn request_kind_value(kind: &SequenceRequestKind, now: Instant) -> Value {
    match kind {
        SequenceRequestKind::Items(request) => json!({
            "type": "itemsRequested",
            "sequenceId": request.sequence_id.as_str(),
            "sessionGeneration": request.session_generation.get(),
            "requestId": request.request_id.get(),
            "direction": direction_wire(request.direction),
            "anchorItemId": request.anchor_item_id.as_ref().map(SequenceItemId::as_str),
            "maxCount": request.max_count,
            "deadlineRemainingMs": duration_millis(request.deadline.saturating_duration_since(now)),
        }),
        SequenceRequestKind::Source(request) => json!({
            "type": "sourceResolutionRequired",
            "sequenceId": request.sequence_id.as_str(),
            "sessionGeneration": request.session_generation.get(),
            "requestId": request.request_id.get(),
            "resolutionAttemptId": request.attempt_id.get(),
            "itemId": request.item_id.as_str(),
            "expectedSourceRevision": request.expected_revision.get(),
            "reason": resolution_reason_wire(request.reason),
            "deadlineRemainingMs": duration_millis(request.deadline.saturating_duration_since(now)),
        }),
    }
}

fn event_value(event: &SequenceEvent, now: Instant) -> Value {
    let payload = match &event.kind {
        SequenceEventKind::SnapshotChanged => json!({ "type": "snapshotChanged" }),
        SequenceEventKind::ActiveItemChanged {
            item_id,
            reason,
            activation_epoch,
        } => json!({
            "type": "activeItemChanged",
            "itemId": item_id.as_str(),
            "reason": activation_reason_wire(*reason),
            "activationEpoch": activation_epoch.get(),
        }),
        SequenceEventKind::Request(request) => request_kind_value(request, now),
        SequenceEventKind::RequestTimedOut(request_id) => json!({
            "type": "requestTimedOut",
            "requestId": request_id.get(),
        }),
        SequenceEventKind::RequestFailed(failure) => {
            let mut value = request_failure_value(failure);
            value["type"] = Value::String("requestFailed".to_owned());
            value
        }
        SequenceEventKind::RequestCancelled(request_id) => json!({
            "type": "requestCancelled",
            "requestId": request_id.get(),
        }),
        SequenceEventKind::SourceAccepted {
            item_id,
            source_revision,
        } => json!({
            "type": "sourceAccepted",
            "itemId": item_id.as_str(),
            "sourceRevision": source_revision.get(),
        }),
        SequenceEventKind::SourceExpired {
            item_id,
            source_revision,
        } => json!({
            "type": "sourceExpired",
            "itemId": item_id.as_str(),
            "sourceRevision": source_revision.get(),
        }),
    };
    json!({
        "eventSequence": event.event_sequence,
        "sessionGeneration": event.session_generation.get(),
        "event": payload,
    })
}

fn request_failure_value(failure: &SequenceRequestFailure) -> Value {
    json!({
        "requestId": failure.request_id.get(),
        "reasonCode": failure.reason_code,
        "timedOut": failure.timed_out,
    })
}

fn preload_intent_value(intent: &SequencePreloadIntent) -> Value {
    json!({
        "sessionGeneration": intent.session_generation.get(),
        "itemId": intent.item_id.as_str(),
        "sourceReference": intent.source_reference.as_str(),
        "sourceRevision": intent.source_revision.get(),
        "warmupTaskId": intent.warmup_task_id.get(),
        "cacheIdentity": cache_identity_value(&intent.cache_identity),
        "priority": match intent.priority {
            player_runtime::SequencePreloadPriority::Current => "current",
            player_runtime::SequencePreloadPriority::Next => "next",
            player_runtime::SequencePreloadPriority::Previous => "previous",
        },
        "warmupGoal": match intent.warmup_goal {
            SequenceWarmupGoal::ProgressiveRange => "progressiveRange",
        },
        "profile": {
            "expectedMemoryBytes": intent.profile.expected_memory_bytes,
            "expectedDiskBytes": intent.profile.expected_disk_bytes,
            "ttlMs": intent.profile.ttl.map(duration_millis),
            "warmupWindowMs": intent.profile.warmup_window.map(duration_millis),
        },
    })
}

fn warmup_task_value(task: &SequenceWarmupTaskSnapshot) -> Value {
    json!({
        "taskId": task.task_id.get(),
        "itemId": task.item_id.as_str(),
        "sourceRevision": task.source_revision.get(),
        "warmupGoal": match task.warmup_goal {
            SequenceWarmupGoal::ProgressiveRange => "progressiveRange",
        },
        "status": warmup_status_wire(task.status),
        "expectedBytes": task.expected_bytes,
        "actualBytes": task.actual_bytes,
        "cacheHit": task.cache_hit,
        "cacheEntries": task.cache_entries,
        "cacheBytes": task.cache_bytes,
        "evictedEntries": task.evicted_entries,
        "reasonCode": task.reason_code,
    })
}

fn cache_identity_value(identity: &SequenceCacheIdentity) -> Value {
    json!({
        "providerNamespace": identity.provider_namespace,
        "contentIdentity": identity.content_identity,
        "renditionIdentity": identity.rendition_identity,
        "resourceIdentity": identity.resource_identity,
        "accessPartition": identity.access_partition,
        "sourceRevision": identity.source_revision.get(),
        "canonicalKey": identity.canonical_key(),
    })
}

fn navigation_value(
    outcome: SequenceNavigationOutcome,
) -> Result<Value, MobileSequenceBridgeError> {
    Ok(match outcome {
        SequenceNavigationOutcome::Activated {
            item_id,
            activation_epoch,
        } => json!({
            "outcome": "activated",
            "itemId": item_id.as_str(),
            "activationEpoch": activation_epoch.get(),
        }),
        SequenceNavigationOutcome::ReachedEnd => json!({ "outcome": "reachedEnd" }),
        SequenceNavigationOutcome::AwaitingItems(request_id) => json!({
            "outcome": "awaitingItems",
            "requestId": request_id.get(),
        }),
        SequenceNavigationOutcome::Empty => json!({ "outcome": "empty" }),
    })
}

fn ensure_json_size(value: &str) -> Result<(), MobileSequenceBridgeError> {
    if value.len() > MAX_MOBILE_SEQUENCE_JSON_BYTES {
        Err(MobileSequenceBridgeError::new(
            "capacity_exceeded",
            "sequence JSON exceeded 512 KiB",
        ))
    } else {
        Ok(())
    }
}

fn ensure_batch_len(len: usize) -> Result<(), MobileSequenceBridgeError> {
    if len > MAX_MOBILE_SEQUENCE_BATCH_ITEMS {
        Err(MobileSequenceBridgeError::new(
            "capacity_exceeded",
            "sequence item batch exceeded 512 items",
        ))
    } else {
        Ok(())
    }
}

fn normalize_optional_reference(
    value: Option<String>,
) -> Result<Option<String>, MobileSequenceBridgeError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > 512 || value.chars().any(char::is_control) || value.contains("://") {
        return Err(MobileSequenceBridgeError::new(
            "invalid_argument",
            "provider metadata reference must be a bounded opaque reference",
        ));
    }
    Ok(Some(value))
}

fn ok_envelope(value: Value) -> String {
    json!({ "ok": true, "result": value }).to_string()
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn media_kind_from_wire(value: &str) -> Result<SequenceMediaKind, MobileSequenceBridgeError> {
    match value {
        "vod" => Ok(SequenceMediaKind::Vod),
        "live" => Ok(SequenceMediaKind::Live),
        "liveDvr" => Ok(SequenceMediaKind::LiveDvr),
        _ => Err(MobileSequenceBridgeError::new(
            "unknown_enum",
            "mediaKind must be vod, live, or liveDvr",
        )),
    }
}

fn warmup_goal_from_wire(value: &str) -> Result<SequenceWarmupGoal, MobileSequenceBridgeError> {
    match value {
        "progressiveRange" => Ok(SequenceWarmupGoal::ProgressiveRange),
        _ => Err(MobileSequenceBridgeError::new(
            "unknown_enum",
            format!("unknown warmup goal: {value}"),
        )),
    }
}

fn warmup_status_from_wire(value: &str) -> Result<SequenceWarmupStatus, MobileSequenceBridgeError> {
    match value {
        "started" => Ok(SequenceWarmupStatus::Started),
        "completed" => Ok(SequenceWarmupStatus::Completed),
        "cancelled" => Ok(SequenceWarmupStatus::Cancelled),
        "failed" => Ok(SequenceWarmupStatus::Failed),
        "unsupported" => Ok(SequenceWarmupStatus::Unsupported),
        _ => Err(MobileSequenceBridgeError::new(
            "unknown_enum",
            format!("unknown warmup status: {value}"),
        )),
    }
}

fn warmup_status_wire(status: SequenceWarmupStatus) -> &'static str {
    match status {
        SequenceWarmupStatus::Started => "started",
        SequenceWarmupStatus::Completed => "completed",
        SequenceWarmupStatus::Cancelled => "cancelled",
        SequenceWarmupStatus::Failed => "failed",
        SequenceWarmupStatus::Unsupported => "unsupported",
    }
}

fn media_kind_wire(value: SequenceMediaKind) -> &'static str {
    match value {
        SequenceMediaKind::Vod => "vod",
        SequenceMediaKind::Live => "live",
        SequenceMediaKind::LiveDvr => "liveDvr",
    }
}

fn direction_wire(value: SequenceDirection) -> &'static str {
    match value {
        SequenceDirection::Previous => "previous",
        SequenceDirection::Next => "next",
    }
}

fn resolution_reason_wire(value: SequenceSourceResolutionReason) -> &'static str {
    match value {
        SequenceSourceResolutionReason::Initial => "initial",
        SequenceSourceResolutionReason::Expired => "expired",
        SequenceSourceResolutionReason::HostRejected => "hostRejected",
        SequenceSourceResolutionReason::Refresh => "refresh",
    }
}

fn activation_reason_wire(value: player_runtime::SequenceActivationReason) -> &'static str {
    match value {
        player_runtime::SequenceActivationReason::Replace => "replace",
        player_runtime::SequenceActivationReason::SetActive => "setActive",
        player_runtime::SequenceActivationReason::Next => "next",
        player_runtime::SequenceActivationReason::Previous => "previous",
        player_runtime::SequenceActivationReason::Removal => "removal",
    }
}

fn sequence_error_code(value: SequenceErrorCode) -> &'static str {
    match value {
        SequenceErrorCode::InvalidArgument => "invalid_argument",
        SequenceErrorCode::DuplicateItemId => "duplicate_item_id",
        SequenceErrorCode::CapacityExceeded => "capacity_exceeded",
        SequenceErrorCode::ItemNotFound => "item_not_found",
        SequenceErrorCode::StaleGeneration => "stale_generation",
        SequenceErrorCode::UnknownRequest => "unknown_request",
        SequenceErrorCode::RequestMismatch => "request_mismatch",
        SequenceErrorCode::StaleSource => "stale_source",
        SequenceErrorCode::StaleActivation => "stale_activation",
        SequenceErrorCode::InvalidState => "invalid_state",
    }
}

fn default_sequence_mode() -> String {
    "finite".to_owned()
}

fn default_history_limit() -> usize {
    SequenceConfig::default().history_limit
}

fn default_forward_window() -> usize {
    SequenceConfig::default().forward_window
}

fn default_refill_threshold() -> usize {
    SequenceConfig::default().refill_threshold
}

fn default_max_items() -> usize {
    SequenceConfig::default()
        .max_items
        .min(MAX_MOBILE_SEQUENCE_BATCH_ITEMS)
}

fn default_max_pending_requests() -> usize {
    SequenceConfig::default().max_pending_requests
}

fn default_max_events() -> usize {
    SequenceConfig::default()
        .max_events
        .min(MAX_MOBILE_SEQUENCE_BATCH_ITEMS * 2)
}

fn default_request_timeout_ms() -> u64 {
    duration_millis(SequenceConfig::default().request_timeout)
}

fn default_source_expiry_lead_ms() -> u64 {
    duration_millis(SequenceConfig::default().source_expiry_lead)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> &'static str {
        r#"{"sequenceId":"feed","mode":"replenishable","maxEvents":16}"#
    }

    fn resolved_item_json(id: &str, revision: u64) -> String {
        json!({
            "itemId": id,
            "providerNamespace": "example.provider",
            "contentIdentity": format!("content-{id}"),
            "mediaKind": "vod",
            "resolvedSource": {
                "sourceReference": format!("source-{id}-{revision}"),
                "cacheIdentity": {
                    "providerNamespace": "example.provider",
                    "contentIdentity": format!("content-{id}"),
                    "renditionIdentity": "1080p",
                    "resourceIdentity": "media",
                    "accessPartition": "public",
                    "sourceRevision": revision,
                }
            }
        })
        .to_string()
    }

    #[test]
    fn bridge_executes_progressive_two_item_slice_without_urls() {
        let mut session = MobileSequenceBridgeSession::from_config_json(config()).expect("config");
        let replace = format!(
            r#"{{"type":"replace","items":[{},{}],"activeItemId":"a"}}"#,
            resolved_item_json("a", 1),
            resolved_item_json("b", 1),
        );
        let replace_result = session.execute_json(&replace, 1_000);
        assert!(replace_result.contains(r#""ok":true"#));
        assert!(
            session
                .execute_json(r#"{"type":"next"}"#, 1_000)
                .contains(r#""itemId":"b""#)
        );
        assert!(
            session
                .execute_json(r#"{"type":"previous"}"#, 1_000)
                .contains(r#""itemId":"a""#)
        );
        let snapshot = session.snapshot_json();
        assert!(!snapshot.contains("https://"));
        assert!(!snapshot.contains("headers"));
    }

    #[test]
    fn bridge_rejects_unknown_enums_and_oversized_batches() {
        let mut session = MobileSequenceBridgeSession::from_config_json(config()).expect("config");
        let invalid = r#"{"type":"replace","items":[{"itemId":"a","providerNamespace":"example.provider","contentIdentity":"a","mediaKind":"unknown"}]}"#;
        assert!(
            session
                .execute_json(invalid, 1_000)
                .contains("unknown_enum")
        );

        let oversized = "x".repeat(MAX_MOBILE_SEQUENCE_JSON_BYTES + 1);
        assert!(
            session
                .execute_json(&oversized, 1_000)
                .contains("capacity_exceeded")
        );
    }

    #[test]
    fn bridge_event_drain_is_bounded_and_pending_requests_resync() {
        let mut session = MobileSequenceBridgeSession::from_config_json(config()).expect("config");
        let item = r#"{"itemId":"a","providerNamespace":"example.provider","contentIdentity":"a","mediaKind":"vod"}"#;
        let replace = format!(r#"{{"type":"replace","items":[{item}],"activeItemId":"a"}}"#);
        let replace_result = session.execute_json(&replace, 1_000);
        assert!(replace_result.contains(r#""ok":true"#));
        assert!(session.snapshot_json().contains("sourceResolutionRequired"));
        assert!(session.drain_events_json(1).contains(r#""events":["#));
        assert!(
            session
                .execute_json(r#"{"type":"resyncPendingRequests"}"#, 1_000)
                .contains(r#""ok":true"#)
        );
        assert!(
            session
                .drain_events_json(512)
                .contains("sourceResolutionRequired")
        );
    }

    #[test]
    fn bridge_round_trips_warmup_reports_and_rejects_stale_or_unknown_values() {
        let mut session = MobileSequenceBridgeSession::from_config_json(config()).expect("config");
        let replace = format!(
            r#"{{"type":"replace","items":[{}],"activeItemId":"a"}}"#,
            resolved_item_json("a", 1),
        );
        assert!(
            session
                .execute_json(&replace, 1_000)
                .contains(r#""ok":true"#)
        );
        let preload: Value =
            serde_json::from_str(&session.preload_intents_json(1_000)).expect("preload JSON");
        let intent = &preload["result"]["intents"][0];
        let generation = intent["sessionGeneration"]
            .as_u64()
            .expect("session generation");
        let task_id = intent["warmupTaskId"].as_u64().expect("warmup task id");
        let report = json!({
            "type": "reportWarmup",
            "sessionGeneration": generation,
            "taskId": task_id,
            "itemId": "a",
            "sourceRevision": 1,
            "warmupGoal": "progressiveRange",
            "status": "completed",
            "expectedBytes": 65_536,
            "actualBytes": 65_536,
            "cacheHit": false,
            "cacheEntries": 1,
            "cacheBytes": 65_536,
            "evictedEntries": 0,
        });
        assert!(
            session
                .execute_json(&report.to_string(), 1_000)
                .contains(r#""accepted":true"#)
        );
        let snapshot = session.snapshot_json();
        assert!(snapshot.contains(r#""warmupTasks":[{"#));
        assert!(snapshot.contains(r#""completed":1"#));

        let mut unknown_goal = report.clone();
        unknown_goal["warmupGoal"] = Value::String("decoderReady".to_owned());
        assert!(
            session
                .execute_json(&unknown_goal.to_string(), 1_000)
                .contains("unknown_enum")
        );
        let mut unknown_status = report.clone();
        unknown_status["status"] = Value::String("futureStatus".to_owned());
        assert!(
            session
                .execute_json(&unknown_status.to_string(), 1_000)
                .contains("unknown_enum")
        );

        assert!(
            session
                .execute_json(&replace, 1_000)
                .contains(r#""ok":true"#)
        );
        assert!(
            session
                .execute_json(&report.to_string(), 1_000)
                .contains("stale_generation")
        );
    }
}
