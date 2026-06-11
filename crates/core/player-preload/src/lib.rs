#![deny(unsafe_code)]
//! Preload planning primitives for media warmup.
//!
//! This crate keeps preload decisions independent from a concrete downloader or
//! cache implementation. Callers provide budget and executor adapters, then feed
//! candidates into a [`PreloadPlanner`] to receive task snapshots and lifecycle
//! events.

mod preload;

pub use preload::{
    DEFAULT_PRELOAD_MAX_CONCURRENT_TASKS, DEFAULT_PRELOAD_MAX_DISK_BYTES,
    DEFAULT_PRELOAD_MAX_MEMORY_BYTES, DEFAULT_PRELOAD_WARMUP_WINDOW, InMemoryPreloadBudgetProvider,
    InMemoryPreloadExecutor, PlayerPreloadBudgetPolicy, PlayerResolvedPreloadBudgetPolicy,
    PreloadBudget, PreloadBudgetProvider, PreloadBudgetScope, PreloadCacheKey, PreloadCandidate,
    PreloadCandidateKind, PreloadConfig, PreloadErrorSummary, PreloadEvent, PreloadExecutor,
    PreloadPlanner, PreloadPriority, PreloadSelectionHint, PreloadSnapshot, PreloadSourceIdentity,
    PreloadTaskId, PreloadTaskSnapshot, PreloadTaskState, PreloadTaskStatus,
    preload_candidate_precedes_or_equal,
};
