//! Shared playback policies and resolution functions.

use std::time::Duration;

use player_model::{
    MediaAbrMode, MediaAbrPolicy, MediaSourceKind, MediaSourceProtocol, MediaTrackSelection,
    MediaTrackSelectionMode,
};
use player_preload::{PlayerPreloadBudgetPolicy, PlayerResolvedPreloadBudgetPolicy};

use crate::{DEFAULT_RETRY_BASE_DELAY, DEFAULT_RETRY_MAX_DELAY};

/// Named buffering presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerBufferingPreset {
    /// Resolve from source kind and protocol.
    Default,
    /// Balanced default for ordinary playback.
    Balanced,
    /// Larger buffers for remote progressive playback.
    Streaming,
    /// Larger buffers for streaming protocols.
    Resilient,
    /// Smaller buffers for latency-sensitive playback.
    LowLatency,
}

/// Buffering policy with optional explicit duration overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerBufferingPolicy {
    /// Preset used as the base policy.
    pub preset: PlayerBufferingPreset,
    /// Minimum desired buffered duration.
    pub min_buffer: Option<Duration>,
    /// Maximum desired buffered duration.
    pub max_buffer: Option<Duration>,
    /// Buffer required before initial playback.
    pub buffer_for_playback: Option<Duration>,
    /// Buffer required before resuming after rebuffering.
    pub buffer_for_rebuffer: Option<Duration>,
}

/// Retry backoff shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerRetryBackoff {
    /// Use the same delay for every retry.
    Fixed,
    /// Increase delay linearly.
    Linear,
    /// Increase delay exponentially.
    Exponential,
}

/// Retry policy used by runtimes that support recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRetryPolicy {
    /// Maximum retry attempts, or unlimited when `None`.
    pub max_attempts: Option<u32>,
    /// Initial retry delay.
    pub base_delay: Duration,
    /// Maximum retry delay.
    pub max_delay: Duration,
    /// Delay growth behavior.
    pub backoff: PlayerRetryBackoff,
}

/// Named cache presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerCachePreset {
    /// Resolve from source kind and protocol.
    Default,
    /// Disable runtime cache use.
    Disabled,
    /// Cache budget for remote progressive playback.
    Streaming,
    /// Larger cache budget for streaming protocols.
    Resilient,
}

/// Cache policy with optional explicit budget overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerCachePolicy {
    /// Preset used as the base policy.
    pub preset: PlayerCachePreset,
    /// Maximum memory bytes available for cache use.
    pub max_memory_bytes: Option<u64>,
    /// Maximum disk bytes available for cache use.
    pub max_disk_bytes: Option<u64>,
}

/// Resolved resilience policies for one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerResolvedResiliencePolicy {
    /// Resolved buffering policy.
    pub buffering_policy: PlayerBufferingPolicy,
    /// Resolved retry policy.
    pub retry_policy: PlayerRetryPolicy,
    /// Resolved cache policy.
    pub cache_policy: PlayerCachePolicy,
}

/// Preferred track selection policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerTrackPreferencePolicy {
    /// Preferred audio language tag.
    pub preferred_audio_language: Option<String>,
    /// Preferred subtitle language tag.
    pub preferred_subtitle_language: Option<String>,
    /// Whether subtitles should be selected by default.
    pub select_subtitles_by_default: bool,
    /// Whether undetermined-language subtitles may be selected by default.
    pub select_undetermined_subtitle_language: bool,
    /// Preferred audio selection.
    pub audio_selection: MediaTrackSelection,
    /// Preferred subtitle selection.
    pub subtitle_selection: MediaTrackSelection,
    /// Preferred adaptive bitrate policy.
    pub abr_policy: MediaAbrPolicy,
}

/// Resolves buffering, retry, and cache policies for one source.
pub fn resolve_resilience_policy(
    source_kind: MediaSourceKind,
    source_protocol: MediaSourceProtocol,
    buffering_policy: PlayerBufferingPolicy,
    retry_policy: PlayerRetryPolicy,
    cache_policy: PlayerCachePolicy,
) -> PlayerResolvedResiliencePolicy {
    PlayerResolvedResiliencePolicy {
        buffering_policy: buffering_policy.resolved_for_source(source_kind, source_protocol),
        retry_policy: retry_policy.resolved(),
        cache_policy: cache_policy.resolved_for_source(source_kind, source_protocol),
    }
}

/// Normalizes track preferences using runtime defaults.
pub fn resolve_track_preferences(
    track_preferences: PlayerTrackPreferencePolicy,
) -> PlayerTrackPreferencePolicy {
    track_preferences.resolved()
}

/// Resolves preload budget overrides using runtime defaults.
pub fn resolve_preload_budget(
    preload_budget: PlayerPreloadBudgetPolicy,
) -> PlayerResolvedPreloadBudgetPolicy {
    preload_budget.resolved()
}

impl PlayerBufferingPolicy {
    /// Returns the buffering policy implied by a source, if any.
    pub fn source_default(
        source_kind: MediaSourceKind,
        source_protocol: MediaSourceProtocol,
    ) -> Option<Self> {
        match source_kind {
            MediaSourceKind::Local => None,
            MediaSourceKind::Remote => match source_protocol {
                MediaSourceProtocol::Hls | MediaSourceProtocol::Dash => Some(Self::resilient()),
                _ => Some(Self::streaming()),
            },
        }
    }

    fn merge_onto(&self, base: Option<&Self>) -> Self {
        Self {
            preset: if self.preset == PlayerBufferingPreset::Default {
                base.map(|policy| policy.preset).unwrap_or(self.preset)
            } else {
                self.preset
            },
            min_buffer: self
                .min_buffer
                .or(base.and_then(|policy| policy.min_buffer)),
            max_buffer: self
                .max_buffer
                .or(base.and_then(|policy| policy.max_buffer)),
            buffer_for_playback: self
                .buffer_for_playback
                .or(base.and_then(|policy| policy.buffer_for_playback)),
            buffer_for_rebuffer: self
                .buffer_for_rebuffer
                .or(base.and_then(|policy| policy.buffer_for_rebuffer)),
        }
    }

    /// Resolves this policy against source-specific defaults.
    pub fn resolved_for_source(
        &self,
        source_kind: MediaSourceKind,
        source_protocol: MediaSourceProtocol,
    ) -> Self {
        let base = match self.preset {
            PlayerBufferingPreset::Default => Self::source_default(source_kind, source_protocol),
            PlayerBufferingPreset::Balanced => Some(Self::balanced()),
            PlayerBufferingPreset::Streaming => Some(Self::streaming()),
            PlayerBufferingPreset::Resilient => Some(Self::resilient()),
            PlayerBufferingPreset::LowLatency => Some(Self::low_latency()),
        };

        self.merge_onto(base.as_ref())
    }

    /// Returns the balanced buffering preset.
    pub fn balanced() -> Self {
        Self {
            preset: PlayerBufferingPreset::Balanced,
            min_buffer: Some(Duration::from_millis(10_000)),
            max_buffer: Some(Duration::from_millis(30_000)),
            buffer_for_playback: Some(Duration::from_millis(1_000)),
            buffer_for_rebuffer: Some(Duration::from_millis(2_000)),
        }
    }

    /// Returns the streaming buffering preset.
    pub fn streaming() -> Self {
        Self {
            preset: PlayerBufferingPreset::Streaming,
            min_buffer: Some(Duration::from_millis(12_000)),
            max_buffer: Some(Duration::from_millis(36_000)),
            buffer_for_playback: Some(Duration::from_millis(1_200)),
            buffer_for_rebuffer: Some(Duration::from_millis(2_500)),
        }
    }

    /// Returns the resilient buffering preset.
    pub fn resilient() -> Self {
        Self {
            preset: PlayerBufferingPreset::Resilient,
            min_buffer: Some(Duration::from_millis(20_000)),
            max_buffer: Some(Duration::from_millis(50_000)),
            buffer_for_playback: Some(Duration::from_millis(1_500)),
            buffer_for_rebuffer: Some(Duration::from_millis(3_000)),
        }
    }

    /// Returns the low-latency buffering preset.
    pub fn low_latency() -> Self {
        Self {
            preset: PlayerBufferingPreset::LowLatency,
            min_buffer: Some(Duration::from_millis(4_000)),
            max_buffer: Some(Duration::from_millis(12_000)),
            buffer_for_playback: Some(Duration::from_millis(500)),
            buffer_for_rebuffer: Some(Duration::from_millis(1_000)),
        }
    }
}

impl Default for PlayerBufferingPolicy {
    fn default() -> Self {
        Self {
            preset: PlayerBufferingPreset::Default,
            min_buffer: None,
            max_buffer: None,
            buffer_for_playback: None,
            buffer_for_rebuffer: None,
        }
    }
}

impl PlayerRetryPolicy {
    /// Returns this retry policy with runtime defaults applied.
    pub fn resolved(&self) -> Self {
        self.clone()
    }

    /// Returns a short retry policy for quick failure.
    pub fn aggressive() -> Self {
        Self {
            max_attempts: Some(2),
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_millis(2_000),
            backoff: PlayerRetryBackoff::Fixed,
        }
    }

    /// Returns a longer retry policy for unstable networks.
    pub fn resilient() -> Self {
        Self {
            max_attempts: Some(6),
            base_delay: Duration::from_millis(1_000),
            max_delay: Duration::from_millis(8_000),
            backoff: PlayerRetryBackoff::Exponential,
        }
    }
}

impl Default for PlayerRetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: Some(3),
            base_delay: DEFAULT_RETRY_BASE_DELAY,
            max_delay: DEFAULT_RETRY_MAX_DELAY,
            backoff: PlayerRetryBackoff::Linear,
        }
    }
}

impl PlayerCachePolicy {
    /// Returns the cache policy implied by a source.
    pub fn source_default(
        source_kind: MediaSourceKind,
        source_protocol: MediaSourceProtocol,
    ) -> Self {
        match source_kind {
            MediaSourceKind::Local => Self::disabled(),
            MediaSourceKind::Remote => match source_protocol {
                MediaSourceProtocol::Hls | MediaSourceProtocol::Dash => Self::resilient(),
                _ => Self::streaming(),
            },
        }
    }

    fn merge_onto(&self, base: &Self) -> Self {
        Self {
            preset: if self.preset == PlayerCachePreset::Default {
                base.preset
            } else {
                self.preset
            },
            max_memory_bytes: self.max_memory_bytes.or(base.max_memory_bytes),
            max_disk_bytes: self.max_disk_bytes.or(base.max_disk_bytes),
        }
    }

    /// Resolves this policy against source-specific defaults.
    pub fn resolved_for_source(
        &self,
        source_kind: MediaSourceKind,
        source_protocol: MediaSourceProtocol,
    ) -> Self {
        if source_kind == MediaSourceKind::Local {
            return Self::disabled();
        }

        let base = match self.preset {
            PlayerCachePreset::Default => Self::source_default(source_kind, source_protocol),
            PlayerCachePreset::Disabled => Self::disabled(),
            PlayerCachePreset::Streaming => Self::streaming(),
            PlayerCachePreset::Resilient => Self::resilient(),
        };

        self.merge_onto(&base)
    }

    /// Returns a policy that disables cache budgets.
    pub fn disabled() -> Self {
        Self {
            preset: PlayerCachePreset::Disabled,
            max_memory_bytes: Some(0),
            max_disk_bytes: Some(0),
        }
    }

    /// Returns the streaming cache preset.
    pub fn streaming() -> Self {
        Self {
            preset: PlayerCachePreset::Streaming,
            max_memory_bytes: Some(8 * 1024 * 1024),
            max_disk_bytes: Some(128 * 1024 * 1024),
        }
    }

    /// Returns the resilient cache preset.
    pub fn resilient() -> Self {
        Self {
            preset: PlayerCachePreset::Resilient,
            max_memory_bytes: Some(16 * 1024 * 1024),
            max_disk_bytes: Some(384 * 1024 * 1024),
        }
    }
}

impl Default for PlayerCachePolicy {
    fn default() -> Self {
        Self {
            preset: PlayerCachePreset::Default,
            max_memory_bytes: None,
            max_disk_bytes: None,
        }
    }
}

impl PlayerTrackPreferencePolicy {
    /// Returns track preferences with empty text and invalid selections normalized.
    pub fn resolved(&self) -> Self {
        Self {
            preferred_audio_language: normalize_optional_text(
                self.preferred_audio_language.as_deref(),
            ),
            preferred_subtitle_language: normalize_optional_text(
                self.preferred_subtitle_language.as_deref(),
            ),
            select_subtitles_by_default: self.select_subtitles_by_default,
            select_undetermined_subtitle_language: self.select_undetermined_subtitle_language,
            audio_selection: normalize_track_selection(
                &self.audio_selection,
                MediaTrackSelection::auto(),
            ),
            subtitle_selection: normalize_track_selection(
                &self.subtitle_selection,
                MediaTrackSelection::disabled(),
            ),
            abr_policy: normalize_abr_policy(&self.abr_policy),
        }
    }
}

impl Default for PlayerTrackPreferencePolicy {
    fn default() -> Self {
        Self {
            preferred_audio_language: None,
            preferred_subtitle_language: None,
            select_subtitles_by_default: false,
            select_undetermined_subtitle_language: false,
            audio_selection: MediaTrackSelection::auto(),
            subtitle_selection: MediaTrackSelection::disabled(),
            abr_policy: MediaAbrPolicy::default(),
        }
    }
}

fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    let normalized = value?.trim();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_owned())
    }
}

fn normalize_track_selection(
    selection: &MediaTrackSelection,
    fallback: MediaTrackSelection,
) -> MediaTrackSelection {
    match selection.mode {
        MediaTrackSelectionMode::Auto => MediaTrackSelection::auto(),
        MediaTrackSelectionMode::Disabled => MediaTrackSelection::disabled(),
        MediaTrackSelectionMode::Track => normalize_optional_text(selection.track_id.as_deref())
            .map(MediaTrackSelection::track)
            .unwrap_or(fallback),
    }
}

fn normalize_abr_policy(policy: &MediaAbrPolicy) -> MediaAbrPolicy {
    match policy.mode {
        MediaAbrMode::Auto => MediaAbrPolicy::default(),
        MediaAbrMode::Constrained => {
            let normalized = MediaAbrPolicy {
                mode: MediaAbrMode::Constrained,
                track_id: None,
                max_bit_rate: policy.max_bit_rate,
                max_width: policy.max_width,
                max_height: policy.max_height,
            };
            if normalized.max_bit_rate.is_none()
                && normalized.max_width.is_none()
                && normalized.max_height.is_none()
            {
                MediaAbrPolicy::default()
            } else {
                normalized
            }
        }
        MediaAbrMode::FixedTrack => normalize_optional_text(policy.track_id.as_deref())
            .map(|track_id| MediaAbrPolicy {
                mode: MediaAbrMode::FixedTrack,
                track_id: Some(track_id),
                max_bit_rate: None,
                max_width: None,
                max_height: None,
            })
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        MediaAbrMode, MediaAbrPolicy, MediaSourceKind, MediaSourceProtocol, MediaTrackSelection,
        MediaTrackSelectionMode, PlayerBufferingPolicy, PlayerCachePolicy,
        PlayerPreloadBudgetPolicy, PlayerRetryBackoff, PlayerRetryPolicy,
        PlayerTrackPreferencePolicy,
    };

    use super::{resolve_preload_budget, resolve_resilience_policy, resolve_track_preferences};

    #[test]
    fn resilience_policy_uses_hls_defaults() {
        let resolved = resolve_resilience_policy(
            MediaSourceKind::Remote,
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
            MediaSourceKind::Remote,
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

    #[test]
    fn resilience_wrapper_preserves_explicit_zero_and_none_values() {
        let resolved = resolve_resilience_policy(
            MediaSourceKind::Remote,
            MediaSourceProtocol::Progressive,
            PlayerBufferingPolicy {
                min_buffer: Some(std::time::Duration::ZERO),
                ..PlayerBufferingPolicy::default()
            },
            PlayerRetryPolicy {
                max_attempts: None,
                ..PlayerRetryPolicy::default()
            },
            PlayerCachePolicy {
                max_memory_bytes: Some(0),
                ..PlayerCachePolicy::default()
            },
        );

        assert_eq!(
            resolved.buffering_policy.min_buffer,
            Some(std::time::Duration::ZERO)
        );
        assert_eq!(
            resolved.buffering_policy.max_buffer,
            Some(std::time::Duration::from_millis(36_000))
        );
        assert_eq!(resolved.retry_policy.max_attempts, None);
        assert_eq!(resolved.cache_policy.max_memory_bytes, Some(0));
        assert_eq!(
            resolved.cache_policy.max_disk_bytes,
            Some(128 * 1024 * 1024)
        );
    }

    #[test]
    fn resilience_wrapper_forces_local_cache_disabled() {
        let resolved = resolve_resilience_policy(
            MediaSourceKind::Local,
            MediaSourceProtocol::File,
            PlayerBufferingPolicy::default(),
            PlayerRetryPolicy::default(),
            PlayerCachePolicy {
                max_memory_bytes: Some(32 * 1024 * 1024),
                max_disk_bytes: Some(512 * 1024 * 1024),
                ..PlayerCachePolicy::default()
            },
        );

        assert_eq!(resolved.cache_policy, PlayerCachePolicy::disabled());
    }

    #[test]
    fn track_wrapper_falls_back_for_invalid_explicit_track_ids() {
        let resolved = resolve_track_preferences(PlayerTrackPreferencePolicy {
            audio_selection: MediaTrackSelection {
                mode: MediaTrackSelectionMode::Track,
                track_id: None,
            },
            subtitle_selection: MediaTrackSelection {
                mode: MediaTrackSelectionMode::Track,
                track_id: Some("   ".to_owned()),
            },
            ..PlayerTrackPreferencePolicy::default()
        });

        assert_eq!(resolved.audio_selection, MediaTrackSelection::auto());
        assert_eq!(resolved.subtitle_selection, MediaTrackSelection::disabled());
    }

    #[test]
    fn preload_wrapper_preserves_explicit_zero_values() {
        let resolved = resolve_preload_budget(PlayerPreloadBudgetPolicy {
            max_concurrent_tasks: Some(0),
            max_memory_bytes: Some(0),
            max_disk_bytes: Some(0),
            warmup_window: Some(std::time::Duration::ZERO),
        });

        assert_eq!(resolved.max_concurrent_tasks, 0);
        assert_eq!(resolved.max_memory_bytes, 0);
        assert_eq!(resolved.max_disk_bytes, 0);
        assert_eq!(resolved.warmup_window, std::time::Duration::ZERO);
    }
}
