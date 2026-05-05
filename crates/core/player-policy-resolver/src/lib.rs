#![deny(unsafe_code)]

use player_runtime::{
    MediaSourceKind, MediaSourceProtocol, PlayerBufferingPolicy, PlayerCachePolicy,
    PlayerPreloadBudgetPolicy, PlayerResolvedPreloadBudgetPolicy, PlayerResolvedResiliencePolicy,
    PlayerRetryPolicy, PlayerRuntimeOptions, PlayerTrackPreferencePolicy,
};

pub fn resolve_resilience_policy(
    source_kind: MediaSourceKind,
    source_protocol: MediaSourceProtocol,
    buffering_policy: PlayerBufferingPolicy,
    retry_policy: PlayerRetryPolicy,
    cache_policy: PlayerCachePolicy,
) -> PlayerResolvedResiliencePolicy {
    PlayerRuntimeOptions::default()
        .with_buffering_policy(buffering_policy)
        .with_retry_policy(retry_policy)
        .with_cache_policy(cache_policy)
        .resolved_resilience_policy(source_kind, source_protocol)
}

pub fn resolve_track_preferences(
    track_preferences: PlayerTrackPreferencePolicy,
) -> PlayerTrackPreferencePolicy {
    PlayerRuntimeOptions::default()
        .with_track_preferences(track_preferences)
        .resolved_track_preferences()
}

pub fn resolve_preload_budget(
    preload_budget: PlayerPreloadBudgetPolicy,
) -> PlayerResolvedPreloadBudgetPolicy {
    PlayerRuntimeOptions::default()
        .with_preload_budget(preload_budget)
        .resolved_preload_budget()
}

#[cfg(test)]
mod tests {
    use player_runtime::{
        MediaAbrMode, MediaAbrPolicy, MediaSourceProtocol, MediaTrackSelection,
        MediaTrackSelectionMode, PlayerBufferingPolicy, PlayerCachePolicy,
        PlayerPreloadBudgetPolicy, PlayerRetryBackoff, PlayerRetryPolicy,
        PlayerTrackPreferencePolicy,
    };

    use super::{resolve_preload_budget, resolve_resilience_policy, resolve_track_preferences};

    #[test]
    fn resilience_policy_uses_hls_defaults() {
        let resolved = resolve_resilience_policy(
            player_runtime::MediaSourceKind::Remote,
            MediaSourceProtocol::Hls,
            PlayerBufferingPolicy::default(),
            PlayerRetryPolicy::default(),
            PlayerCachePolicy::default(),
        );

        assert_eq!(
            resolved.buffering_policy,
            PlayerBufferingPolicy::resilient()
        );
        assert_eq!(resolved.retry_policy, PlayerRetryPolicy::default());
        assert_eq!(resolved.cache_policy, PlayerCachePolicy::resilient());
    }

    #[test]
    fn track_preferences_are_normalized() {
        let resolved = resolve_track_preferences(PlayerTrackPreferencePolicy {
            preferred_audio_language: Some("  ".to_owned()),
            preferred_subtitle_language: Some(" zh-Hans ".to_owned()),
            select_subtitles_by_default: true,
            select_undetermined_subtitle_language: false,
            audio_selection: MediaTrackSelection {
                mode: MediaTrackSelectionMode::Track,
                track_id: Some("  ".to_owned()),
            },
            subtitle_selection: MediaTrackSelection {
                mode: MediaTrackSelectionMode::Track,
                track_id: Some("subtitle-main".to_owned()),
            },
            abr_policy: MediaAbrPolicy {
                mode: MediaAbrMode::Constrained,
                track_id: None,
                max_bit_rate: None,
                max_width: None,
                max_height: None,
            },
        });

        assert_eq!(resolved.preferred_audio_language, None);
        assert_eq!(
            resolved.preferred_subtitle_language,
            Some("zh-Hans".to_owned())
        );
        assert_eq!(resolved.audio_selection, MediaTrackSelection::auto());
        assert_eq!(
            resolved.subtitle_selection,
            MediaTrackSelection {
                mode: MediaTrackSelectionMode::Track,
                track_id: Some("subtitle-main".to_owned()),
            }
        );
        assert_eq!(resolved.abr_policy, MediaAbrPolicy::default());
    }

    #[test]
    fn preload_budget_uses_runtime_defaults() {
        let resolved = resolve_preload_budget(PlayerPreloadBudgetPolicy::default());

        assert_eq!(resolved.max_concurrent_tasks, 2);
        assert_eq!(resolved.max_memory_bytes, 64 * 1024 * 1024);
        assert_eq!(resolved.max_disk_bytes, 256 * 1024 * 1024);
        assert_eq!(
            resolved.warmup_window,
            std::time::Duration::from_millis(30_000)
        );
    }

    #[test]
    fn retry_overrides_are_preserved() {
        let resolved = resolve_resilience_policy(
            player_runtime::MediaSourceKind::Remote,
            MediaSourceProtocol::Progressive,
            PlayerBufferingPolicy::default(),
            PlayerRetryPolicy {
                max_attempts: None,
                base_delay: std::time::Duration::from_millis(2_000),
                max_delay: std::time::Duration::from_millis(8_000),
                backoff: PlayerRetryBackoff::Exponential,
            },
            PlayerCachePolicy::default(),
        );

        assert_eq!(resolved.retry_policy.max_attempts, None);
        assert_eq!(
            resolved.retry_policy.base_delay,
            std::time::Duration::from_millis(2_000)
        );
        assert_eq!(
            resolved.retry_policy.max_delay,
            std::time::Duration::from_millis(8_000)
        );
        assert_eq!(
            resolved.retry_policy.backoff,
            PlayerRetryBackoff::Exponential
        );
    }
}
