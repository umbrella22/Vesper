//! Sequence-to-plugin correlation mapping without runtime side effects.

use std::fmt;

use player_playlist::{
    SequenceErrorCode, SequenceItemSnapshot, SequencePreloadIntent, SequencePreloadPriority,
    SequenceSnapshot, SequenceSourceRevision, SequenceSourceState, SequenceWarmupGoal,
    SequenceWarmupTaskId,
};
use player_plugin::{
    PluginActivePlaybackCorrelation, PluginNextPrewarmCorrelation, PluginPlaybackError,
    PluginSessionCorrelation,
};

/// Typed rejection returned when a sequence intent cannot own a plugin scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerPluginSequenceCorrelationError {
    IntentPriorityMismatch {
        expected: SequencePreloadPriority,
        actual: SequencePreloadPriority,
    },
    SessionGenerationMismatch {
        expected: u64,
        actual: u64,
    },
    SessionIdMismatch,
    ActiveItemMissing,
    ActiveItemMismatch,
    NextItemMismatch,
    ItemMissing,
    SourceUnavailable,
    SourceRevisionMismatch {
        expected: u64,
        actual: u64,
    },
    IntentIdentityMismatch {
        code: SequenceErrorCode,
    },
    Correlation(PluginPlaybackError),
}

impl fmt::Display for PlayerPluginSequenceCorrelationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IntentPriorityMismatch { expected, actual } => write!(
                formatter,
                "sequence preload priority mismatch: expected {expected:?}, got {actual:?}"
            ),
            Self::SessionGenerationMismatch { expected, actual } => write!(
                formatter,
                "sequence session generation mismatch: expected {expected}, got {actual}"
            ),
            Self::SessionIdMismatch => {
                formatter.write_str("sequence session identity does not match the prewarm")
            }
            Self::ActiveItemMissing => formatter.write_str("sequence has no active item"),
            Self::ActiveItemMismatch => {
                formatter.write_str("current preload intent does not identify the active item")
            }
            Self::NextItemMismatch => {
                formatter.write_str("next preload intent does not identify the immediate next item")
            }
            Self::ItemMissing => formatter.write_str("sequence preload item is missing"),
            Self::SourceUnavailable => {
                formatter.write_str("sequence preload item has no resolved source")
            }
            Self::SourceRevisionMismatch { expected, actual } => write!(
                formatter,
                "sequence source revision mismatch: expected {expected}, got {actual}"
            ),
            Self::IntentIdentityMismatch { code } => {
                write!(
                    formatter,
                    "sequence preload intent identity mismatch: {code:?}"
                )
            }
            Self::Correlation(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for PlayerPluginSequenceCorrelationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Correlation(error) => Some(error),
            _ => None,
        }
    }
}

impl From<PluginPlaybackError> for PlayerPluginSequenceCorrelationError {
    fn from(value: PluginPlaybackError) -> Self {
        Self::Correlation(value)
    }
}

/// Pure mapper from authoritative sequence state to bounded plugin identities.
pub struct PlayerPluginSequenceCorrelation;

impl PlayerPluginSequenceCorrelation {
    pub fn active(
        plan_fingerprint: &str,
        snapshot: &SequenceSnapshot,
        intent: &SequencePreloadIntent,
    ) -> Result<PluginActivePlaybackCorrelation, PlayerPluginSequenceCorrelationError> {
        validate_intent(snapshot, intent, SequencePreloadPriority::Current)?;
        let active_item_id = snapshot
            .active_item_id
            .as_ref()
            .ok_or(PlayerPluginSequenceCorrelationError::ActiveItemMissing)?;
        if active_item_id != &intent.item_id {
            return Err(PlayerPluginSequenceCorrelationError::ActiveItemMismatch);
        }
        validate_source(snapshot, intent)?;
        snapshot.validate_preload_intent(intent).map_err(|error| {
            PlayerPluginSequenceCorrelationError::IntentIdentityMismatch { code: error.code }
        })?;
        Ok(PluginActivePlaybackCorrelation::new(
            session(plan_fingerprint, snapshot)?,
            intent.item_id.as_str(),
            intent.source_revision.get(),
            snapshot.activation_epoch.get(),
        )?)
    }

    pub fn next_prewarm(
        plan_fingerprint: &str,
        snapshot: &SequenceSnapshot,
        intent: &SequencePreloadIntent,
    ) -> Result<PluginNextPrewarmCorrelation, PlayerPluginSequenceCorrelationError> {
        validate_intent(snapshot, intent, SequencePreloadPriority::Next)?;
        let active_index = snapshot
            .items
            .iter()
            .position(|item| item.is_active)
            .ok_or(PlayerPluginSequenceCorrelationError::ActiveItemMissing)?;
        if snapshot
            .items
            .get(active_index + 1)
            .map(|item| &item.item.item_id)
            != Some(&intent.item_id)
        {
            return Err(PlayerPluginSequenceCorrelationError::NextItemMismatch);
        }
        validate_source(snapshot, intent)?;
        snapshot.validate_preload_intent(intent).map_err(|error| {
            PlayerPluginSequenceCorrelationError::IntentIdentityMismatch { code: error.code }
        })?;
        Ok(PluginNextPrewarmCorrelation::new(
            session(plan_fingerprint, snapshot)?,
            intent.item_id.as_str(),
            intent.source_revision.get(),
            intent.warmup_task_id.get(),
        )?)
    }

    /// Builds the active identity after a previously validated prewarm becomes active.
    pub fn promoted_active(
        plan_fingerprint: &str,
        snapshot: &SequenceSnapshot,
        prewarm: &PluginNextPrewarmCorrelation,
    ) -> Result<PluginActivePlaybackCorrelation, PlayerPluginSequenceCorrelationError> {
        if prewarm.session().plan_fingerprint() != plan_fingerprint {
            return Err(PlayerPluginSequenceCorrelationError::Correlation(
                PluginPlaybackError::PlanFingerprintMismatch,
            ));
        }
        if prewarm.session().session_id() != snapshot.sequence_id.as_str() {
            return Err(PlayerPluginSequenceCorrelationError::SessionIdMismatch);
        }
        if prewarm.session().session_generation() != snapshot.session_generation.get() {
            return Err(
                PlayerPluginSequenceCorrelationError::SessionGenerationMismatch {
                    expected: snapshot.session_generation.get(),
                    actual: prewarm.session().session_generation(),
                },
            );
        }
        let active_item_id = snapshot
            .active_item_id
            .as_ref()
            .ok_or(PlayerPluginSequenceCorrelationError::ActiveItemMissing)?;
        if active_item_id.as_str() != prewarm.item_id() {
            return Err(PlayerPluginSequenceCorrelationError::ActiveItemMismatch);
        }
        let item = snapshot
            .items
            .iter()
            .find(|item| item.item.item_id == *active_item_id)
            .ok_or(PlayerPluginSequenceCorrelationError::ItemMissing)?;
        let SequenceSourceState::Resolved { revision, .. } = &item.item.source_state else {
            return Err(PlayerPluginSequenceCorrelationError::SourceUnavailable);
        };
        if revision.get() != prewarm.source_revision() {
            return Err(
                PlayerPluginSequenceCorrelationError::SourceRevisionMismatch {
                    expected: revision.get(),
                    actual: prewarm.source_revision(),
                },
            );
        }
        snapshot
            .validate_warmup_task_identity(
                active_item_id,
                SequenceSourceRevision::new(prewarm.source_revision()),
                SequenceWarmupTaskId::new(prewarm.warmup_task_id()),
                SequenceWarmupGoal::ProgressiveRange,
            )
            .map_err(
                |error| PlayerPluginSequenceCorrelationError::IntentIdentityMismatch {
                    code: error.code,
                },
            )?;
        Ok(PluginActivePlaybackCorrelation::new(
            session(plan_fingerprint, snapshot)?,
            prewarm.item_id(),
            prewarm.source_revision(),
            snapshot.activation_epoch.get(),
        )?)
    }
}

fn session(
    plan_fingerprint: &str,
    snapshot: &SequenceSnapshot,
) -> Result<PluginSessionCorrelation, PlayerPluginSequenceCorrelationError> {
    Ok(PluginSessionCorrelation::new(
        plan_fingerprint,
        snapshot.sequence_id.as_str(),
        snapshot.session_generation.get(),
    )?)
}

fn validate_intent(
    snapshot: &SequenceSnapshot,
    intent: &SequencePreloadIntent,
    expected: SequencePreloadPriority,
) -> Result<(), PlayerPluginSequenceCorrelationError> {
    if intent.priority != expected {
        return Err(
            PlayerPluginSequenceCorrelationError::IntentPriorityMismatch {
                expected,
                actual: intent.priority,
            },
        );
    }
    if intent.session_generation != snapshot.session_generation {
        return Err(
            PlayerPluginSequenceCorrelationError::SessionGenerationMismatch {
                expected: snapshot.session_generation.get(),
                actual: intent.session_generation.get(),
            },
        );
    }
    Ok(())
}

fn validate_source<'a>(
    snapshot: &'a SequenceSnapshot,
    intent: &SequencePreloadIntent,
) -> Result<&'a SequenceItemSnapshot, PlayerPluginSequenceCorrelationError> {
    let item = snapshot
        .items
        .iter()
        .find(|item| item.item.item_id == intent.item_id)
        .ok_or(PlayerPluginSequenceCorrelationError::ItemMissing)?;
    let SequenceSourceState::Resolved { revision, .. } = &item.item.source_state else {
        return Err(PlayerPluginSequenceCorrelationError::SourceUnavailable);
    };
    if revision != &intent.source_revision {
        return Err(
            PlayerPluginSequenceCorrelationError::SourceRevisionMismatch {
                expected: revision.get(),
                actual: intent.source_revision.get(),
            },
        );
    }
    Ok(item)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::{PlayerPluginSequenceCorrelation, PlayerPluginSequenceCorrelationError};
    use player_playlist::{
        SequenceCacheIdentity, SequenceClockSnapshot, SequenceConfig, SequenceContentIdentity,
        SequenceCoordinator, SequenceErrorCode, SequenceItem, SequenceItemId, SequenceMediaKind,
        SequencePreloadPriority, SequenceSourceRevision, SequenceWarmupGoal, SequenceWarmupReport,
        SequenceWarmupStatus, SequenceWarmupTaskId,
    };

    const PLAN_FINGERPRINT: &str =
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn clock() -> SequenceClockSnapshot {
        SequenceClockSnapshot {
            wall_epoch_ms: 1_000,
            monotonic: Instant::now(),
        }
    }

    fn item(id: &str, revision: u64) -> SequenceItem {
        SequenceItem::resolved(
            id,
            SequenceContentIdentity::new("dev.vesper.tests", id),
            SequenceMediaKind::Vod,
            format!("source-{id}"),
            SequenceCacheIdentity::new(
                "dev.vesper.tests",
                id,
                "main",
                "manifest",
                "public",
                SequenceSourceRevision::new(revision),
            ),
            None,
        )
    }

    #[test]
    fn sequence_current_and_immediate_next_map_to_distinct_plugin_correlations() {
        let mut coordinator = SequenceCoordinator::new("sequence:scope", SequenceConfig::default())
            .expect("sequence coordinator");
        coordinator
            .replace(
                vec![item("a", 1), item("b", 2), item("c", 3)],
                Some(SequenceItemId::new("a")),
                clock(),
            )
            .expect("replace sequence");
        let snapshot = coordinator.snapshot();
        let intents = coordinator.preload_intents(clock().wall_epoch_ms);
        assert_eq!(intents.len(), 2);

        let active =
            PlayerPluginSequenceCorrelation::active(PLAN_FINGERPRINT, &snapshot, &intents[0])
                .expect("active correlation");
        let next =
            PlayerPluginSequenceCorrelation::next_prewarm(PLAN_FINGERPRINT, &snapshot, &intents[1])
                .expect("next correlation");

        assert_eq!(active.item_id(), "a");
        assert_eq!(active.source_revision(), 1);
        assert_eq!(
            active.playback_generation(),
            snapshot.activation_epoch.get()
        );
        assert_eq!(next.item_id(), "b");
        assert_eq!(next.source_revision(), 2);
        assert_eq!(next.warmup_task_id(), intents[1].warmup_task_id.get());
        assert!(matches!(
            PlayerPluginSequenceCorrelation::active(PLAN_FINGERPRINT, &snapshot, &intents[1],),
            Err(
                PlayerPluginSequenceCorrelationError::IntentPriorityMismatch {
                    expected: SequencePreloadPriority::Current,
                    actual: SequencePreloadPriority::Next,
                }
            )
        ));
    }

    #[test]
    fn sequence_mapping_rejects_stale_generation_revision_and_non_next_item() {
        let mut coordinator = SequenceCoordinator::new("sequence:stale", SequenceConfig::default())
            .expect("sequence coordinator");
        coordinator
            .replace(
                vec![item("a", 1), item("b", 2), item("c", 3)],
                Some(SequenceItemId::new("a")),
                clock(),
            )
            .expect("first sequence");
        let old_intents = coordinator.preload_intents(clock().wall_epoch_ms);

        coordinator
            .replace(
                vec![item("a", 1), item("b", 2), item("c", 3)],
                Some(SequenceItemId::new("a")),
                clock(),
            )
            .expect("new sequence generation");
        let snapshot = coordinator.snapshot();
        assert!(matches!(
            PlayerPluginSequenceCorrelation::next_prewarm(
                PLAN_FINGERPRINT,
                &snapshot,
                &old_intents[1],
            ),
            Err(PlayerPluginSequenceCorrelationError::SessionGenerationMismatch { .. })
        ));

        let mut intents = coordinator.preload_intents(clock().wall_epoch_ms);
        intents[1].source_revision = SequenceSourceRevision::new(99);
        assert_eq!(
            PlayerPluginSequenceCorrelation::next_prewarm(PLAN_FINGERPRINT, &snapshot, &intents[1],),
            Err(
                PlayerPluginSequenceCorrelationError::SourceRevisionMismatch {
                    expected: 2,
                    actual: 99,
                }
            )
        );

        let mut non_next = intents[1].clone();
        non_next.item_id = SequenceItemId::new("c");
        non_next.source_revision = SequenceSourceRevision::new(3);
        assert_eq!(
            PlayerPluginSequenceCorrelation::next_prewarm(PLAN_FINGERPRINT, &snapshot, &non_next,),
            Err(PlayerPluginSequenceCorrelationError::NextItemMismatch)
        );

        let mut forged_task = coordinator.preload_intents(clock().wall_epoch_ms)[1].clone();
        forged_task.warmup_task_id =
            SequenceWarmupTaskId::new(forged_task.warmup_task_id.get().saturating_add(1));
        assert_eq!(
            PlayerPluginSequenceCorrelation::next_prewarm(
                PLAN_FINGERPRINT,
                &snapshot,
                &forged_task,
            ),
            Err(
                PlayerPluginSequenceCorrelationError::IntentIdentityMismatch {
                    code: SequenceErrorCode::StaleSource,
                }
            )
        );
    }

    #[test]
    fn completed_next_prewarm_promotes_without_a_reissued_current_intent() {
        let mut coordinator =
            SequenceCoordinator::new("sequence:promote", SequenceConfig::default())
                .expect("sequence coordinator");
        coordinator
            .replace(
                vec![item("a", 1), item("b", 2), item("c", 3)],
                Some(SequenceItemId::new("a")),
                clock(),
            )
            .expect("replace sequence");
        let snapshot = coordinator.snapshot();
        let intents = coordinator.preload_intents(clock().wall_epoch_ms);
        let next =
            PlayerPluginSequenceCorrelation::next_prewarm(PLAN_FINGERPRINT, &snapshot, &intents[1])
                .expect("next prewarm correlation");
        coordinator
            .report_warmup(SequenceWarmupReport {
                session_generation: intents[1].session_generation,
                task_id: intents[1].warmup_task_id,
                item_id: intents[1].item_id.clone(),
                source_revision: intents[1].source_revision,
                warmup_goal: SequenceWarmupGoal::ProgressiveRange,
                status: SequenceWarmupStatus::Completed,
                expected_bytes: 0,
                actual_bytes: 0,
                cache_hit: None,
                cache_entries: 0,
                cache_bytes: 0,
                evicted_entries: 0,
                reason_code: None,
            })
            .expect("complete next warmup");
        coordinator.next(clock()).expect("activate next item");
        let promoted_snapshot = coordinator.snapshot();
        assert!(
            coordinator
                .preload_intents(clock().wall_epoch_ms)
                .iter()
                .all(|intent| intent.item_id.as_str() != "b")
        );

        let forged = player_plugin::PluginNextPrewarmCorrelation::new(
            next.session().clone(),
            next.item_id(),
            next.source_revision(),
            next.warmup_task_id().saturating_add(1),
        )
        .expect("forged task correlation fixture");
        assert_eq!(
            PlayerPluginSequenceCorrelation::promoted_active(
                PLAN_FINGERPRINT,
                &promoted_snapshot,
                &forged,
            ),
            Err(
                PlayerPluginSequenceCorrelationError::IntentIdentityMismatch {
                    code: SequenceErrorCode::StaleSource,
                }
            )
        );

        let active = PlayerPluginSequenceCorrelation::promoted_active(
            PLAN_FINGERPRINT,
            &promoted_snapshot,
            &next,
        )
        .expect("promoted active correlation");
        assert_eq!(active.item_id(), "b");
        assert_eq!(active.source_revision(), 2);
        assert_eq!(
            active.playback_generation(),
            promoted_snapshot.activation_epoch.get()
        );
    }
}
