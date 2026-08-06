use std::sync::{Arc, Mutex, OnceLock};

use jni::Env;
use jni::sys::jlong;
use player_platform_android::{AndroidHostBridgeSession, AndroidNativeFramePipelineSession};
use player_platform_mobile::MobileSourceNormalizerResourceOpen;
use player_plugin::PluginReference;
use player_plugin_loader::{BenchmarkSinkPluginSession, PluginRegistry};
use player_runtime::{
    MediaSourceKind, MediaSourceProtocol, PlayerBufferingPolicy, PlayerCachePolicy,
    PlayerPreloadBudgetPolicy, PlayerResolvedPreloadBudgetPolicy, PlayerResolvedResiliencePolicy,
    PlayerRetryPolicy, PlayerTrackPreferencePolicy,
    policy::{
        resolve_preload_budget as resolve_preload_budget_via_shared_resolver,
        resolve_resilience_policy as resolve_resilience_policy_via_shared_resolver,
        resolve_track_preferences as resolve_track_preferences_via_shared_resolver,
    },
};

use crate::{HandleRegistry, jni_name, lock_or_recover};

pub(crate) type AndroidJniSession = Arc<Mutex<AndroidHostBridgeSession>>;
/// Benchmark sink sessions invoke plugin FFI calls (`on_event_batch`/`flush`)
/// that may block on a dlopen-loaded plugin. They are stored behind an
/// `Arc<Mutex<...>>` so accessors clone the `Arc` under the global registry
/// lock, drop the registry lock, and only then take the per-session lock while
/// running caller closures. This keeps plugin FFI off the global registry
/// mutex, mirroring `with_session_mut` / `with_source_normalizer_resource_session_mut`.
pub(crate) struct AndroidBenchmarkSinkSessionState {
    session: BenchmarkSinkPluginSession,
    _registry: Option<Arc<PluginRegistry>>,
}
pub(crate) type AndroidBenchmarkSinkSession = Arc<Mutex<AndroidBenchmarkSinkSessionState>>;

/// Source-normalizer resource sessions wrap blocking plugin `poll()` calls that
/// perform filesystem I/O (directory walks, disk-usage scans). They are stored
/// behind an `Arc<Mutex<...>>` so accessors can clone the `Arc` under the global
/// registry lock, drop the registry lock, and only then take the per-session
/// lock while running caller closures. This keeps blocking I/O off the global
/// registry mutex, mirroring `with_session_mut` /
/// `with_native_frame_pipeline_session_mut`.
pub(crate) type AndroidSourceNormalizerResourceSession =
    Arc<Mutex<MobileSourceNormalizerResourceOpen>>;

static SESSIONS: OnceLock<Mutex<HandleRegistry<AndroidJniSession>>> = OnceLock::new();
static BENCHMARK_SINK_SESSIONS: OnceLock<Mutex<HandleRegistry<AndroidBenchmarkSinkSession>>> =
    OnceLock::new();
static SOURCE_NORMALIZER_RESOURCE_SESSIONS: OnceLock<
    Mutex<HandleRegistry<AndroidSourceNormalizerResourceSession>>,
> = OnceLock::new();
static NATIVE_FRAME_PIPELINE_SESSIONS: OnceLock<
    Mutex<HandleRegistry<Arc<Mutex<AndroidNativeFramePipelineSession>>>>,
> = OnceLock::new();

pub(crate) fn sessions() -> &'static Mutex<HandleRegistry<AndroidJniSession>> {
    SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

fn benchmark_sink_sessions() -> &'static Mutex<HandleRegistry<AndroidBenchmarkSinkSession>> {
    BENCHMARK_SINK_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

fn source_normalizer_resource_sessions()
-> &'static Mutex<HandleRegistry<AndroidSourceNormalizerResourceSession>> {
    SOURCE_NORMALIZER_RESOURCE_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

fn native_frame_pipeline_sessions()
-> &'static Mutex<HandleRegistry<Arc<Mutex<AndroidNativeFramePipelineSession>>>> {
    NATIVE_FRAME_PIPELINE_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

fn invalid_handle_error() -> &'static str {
    "invalid android JNI session handle"
}

fn invalid_benchmark_sink_handle_error() -> &'static str {
    "invalid android benchmark sink session handle"
}

fn invalid_source_normalizer_resource_handle_error() -> &'static str {
    "invalid android source normalizer resource session handle"
}

fn invalid_native_frame_pipeline_handle_error() -> &'static str {
    "invalid android native-frame pipeline session handle"
}

pub(crate) fn with_session_mut<R>(
    env: &mut Env<'_>,
    handle: jlong,
    f: impl FnOnce(&mut AndroidHostBridgeSession) -> R,
) -> Option<R> {
    let session = {
        let guard = lock_or_recover(sessions());
        let Some(session) = guard.get(handle).cloned() else {
            let _ = env.throw_new(
                jni_name("java/lang/IllegalArgumentException"),
                jni_name(invalid_handle_error()),
            );
            return None;
        };
        session
    };

    // Do not call `env.call_*` or trigger JNI-reentrant teardown while the session lock is held.
    let mut session = lock_or_recover(session.as_ref());
    Some(f(&mut session))
}

/// Runs a session operation without raising a Java exception for a stale handle.
///
/// Subtitle commands use this form because their public contract returns a
/// structured failure payload through JNI. Other legacy JNI entry points keep
/// using [`with_session_mut`] and its Java exception behavior.
pub(crate) fn with_session_mut_checked<R>(
    handle: jlong,
    f: impl FnOnce(&mut AndroidHostBridgeSession) -> R,
) -> Result<R, &'static str> {
    let session = {
        let guard = lock_or_recover(sessions());
        guard
            .get(handle)
            .cloned()
            .ok_or_else(invalid_handle_error)?
    };

    let mut session = lock_or_recover(session.as_ref());
    Ok(f(&mut session))
}

pub(crate) fn new_session_with_plugin_registry(
    source_uri: String,
    registry: Option<Arc<PluginRegistry>>,
    references: Vec<PluginReference>,
) -> Result<jlong, String> {
    let session = match registry {
        Some(registry) => {
            AndroidHostBridgeSession::new_with_plugin_registry(source_uri, registry, references)
                .map_err(|error| error.to_string())?
        }
        None if references.is_empty() => AndroidHostBridgeSession::new(source_uri),
        None => {
            return Err(
                "Android plugin registry handle is required for playback event-hook references"
                    .to_owned(),
            );
        }
    };
    let mut guard = lock_or_recover(sessions());
    let handle = guard.insert(Arc::new(Mutex::new(session)));
    if handle == 0 {
        return Err("android JNI session registry overflow".to_owned());
    }
    Ok(handle)
}

pub(crate) fn new_benchmark_sink_session_from_registry(
    registry: Arc<PluginRegistry>,
    references: Vec<PluginReference>,
) -> Result<jlong, String> {
    let session = BenchmarkSinkPluginSession::from_registry(&registry, references)
        .map_err(|error| error.to_string())?;
    register_benchmark_sink_session(AndroidBenchmarkSinkSessionState {
        session,
        _registry: Some(registry),
    })
}

fn register_benchmark_sink_session(
    session: AndroidBenchmarkSinkSessionState,
) -> Result<jlong, String> {
    let mut guard = lock_or_recover(benchmark_sink_sessions());
    let handle = guard.insert(Arc::new(Mutex::new(session)));
    if handle == 0 {
        return Err("android benchmark sink session registry overflow".to_owned());
    }
    Ok(handle)
}

pub(crate) fn dispose_benchmark_sink_session(handle: jlong) {
    // Remove the entry under the registry lock, but drop the returned `Arc`
    // (and therefore the inner `BenchmarkSinkPluginSession`, whose `Drop`
    // invokes plugin FFI `destroy`) *after* releasing the registry lock.
    // Dropping it under the lock would serialize every benchmark session
    // process-wide and block new/dispose for the plugin call duration.
    let session = {
        let mut guard = lock_or_recover(benchmark_sink_sessions());
        guard.remove(handle)
    };
    drop(session);
}

pub(crate) fn new_source_normalizer_resource_session(
    session: MobileSourceNormalizerResourceOpen,
) -> Result<jlong, String> {
    let mut guard = lock_or_recover(source_normalizer_resource_sessions());
    let handle = guard.insert(Arc::new(Mutex::new(session)));
    if handle == 0 {
        return Err("android source normalizer resource session registry overflow".to_owned());
    }
    Ok(handle)
}

pub(crate) fn dispose_source_normalizer_resource_session(handle: jlong) {
    // Remove under the registry lock; drop the returned `Arc` (whose inner
    // `MobileSourceNormalizerResourceOpen::Drop` calls plugin FFI
    // `close_resource_session`) *after* releasing the registry lock.
    let session = {
        let mut guard = lock_or_recover(source_normalizer_resource_sessions());
        guard.remove(handle)
    };
    drop(session);
}

pub(crate) fn new_native_frame_pipeline_session(
    session: AndroidNativeFramePipelineSession,
) -> Result<jlong, String> {
    let mut guard = lock_or_recover(native_frame_pipeline_sessions());
    let handle = guard.insert(Arc::new(Mutex::new(session)));
    if handle == 0 {
        return Err("android native-frame pipeline session registry overflow".to_owned());
    }
    Ok(handle)
}

pub(crate) fn dispose_native_frame_pipeline_session(handle: jlong) {
    // Remove under the registry lock; drop the returned `Arc` (whose inner
    // `AndroidNativeFramePipelineSession::Drop` runs plugin FFI close on the
    // processor chain / decoder / presenter / packet source) *after* releasing
    // the registry lock.
    let session = {
        let mut guard = lock_or_recover(native_frame_pipeline_sessions());
        guard.remove(handle)
    };
    drop(session);
}

pub(crate) fn with_native_frame_pipeline_session_mut<R>(
    env: &mut Env<'_>,
    handle: jlong,
    f: impl FnOnce(&mut AndroidNativeFramePipelineSession) -> Result<R, String>,
) -> Option<R> {
    let session = {
        let guard = lock_or_recover(native_frame_pipeline_sessions());
        let Some(session) = guard.get(handle).cloned() else {
            let _ = env.throw_new(
                jni_name("java/lang/IllegalArgumentException"),
                jni_name(invalid_native_frame_pipeline_handle_error()),
            );
            return None;
        };
        session
    };

    let mut session = lock_or_recover(session.as_ref());
    match f(&mut session) {
        Ok(value) => Some(value),
        Err(message) => {
            let _ = env.throw_new(
                jni_name("java/lang/IllegalStateException"),
                jni_name(message),
            );
            None
        }
    }
}

pub(crate) fn with_source_normalizer_resource_session_mut<R>(
    env: &mut Env<'_>,
    handle: jlong,
    f: impl FnOnce(&mut MobileSourceNormalizerResourceOpen) -> Result<R, String>,
) -> Option<R> {
    // Clone the Arc under the global registry lock, then drop the registry lock
    // before invoking `f`. The plugin's `session.poll()` performs blocking
    // filesystem I/O (directory walks, disk-usage scans); holding the global
    // registry mutex across that I/O would serialize every source-normalizer
    // session process-wide and block `new_*` / `dispose_*` calls.
    let session = {
        let guard = lock_or_recover(source_normalizer_resource_sessions());
        let Some(session) = guard.get(handle).cloned() else {
            let _ = env.throw_new(
                jni_name("java/lang/IllegalArgumentException"),
                jni_name(invalid_source_normalizer_resource_handle_error()),
            );
            return None;
        };
        session
    };

    let mut session = lock_or_recover(session.as_ref());
    match f(&mut session) {
        Ok(value) => Some(value),
        Err(message) => {
            let _ = env.throw_new(
                jni_name("java/lang/IllegalStateException"),
                jni_name(message),
            );
            None
        }
    }
}

pub(crate) fn with_benchmark_sink_session<R>(
    env: &mut Env<'_>,
    handle: jlong,
    f: impl FnOnce(&BenchmarkSinkPluginSession) -> Result<R, String>,
) -> Option<R> {
    // Clone the Arc under the global registry lock, then drop the registry lock
    // before invoking `f`. The benchmark sink closure performs plugin FFI calls
    // (`on_event_batch`/`flush`) into dlopen-loaded plugins; holding the global
    // registry mutex across those calls would serialize every benchmark sink
    // session process-wide and block registry mutations for the call duration.
    let session = {
        let guard = lock_or_recover(benchmark_sink_sessions());
        let Some(session) = guard.get(handle).cloned() else {
            let _ = env.throw_new(
                jni_name("java/lang/IllegalArgumentException"),
                jni_name(invalid_benchmark_sink_handle_error()),
            );
            return None;
        };
        session
    };

    let session = lock_or_recover(session.as_ref());
    match f(&session.session) {
        Ok(value) => Some(value),
        Err(message) => {
            let _ = env.throw_new(
                jni_name("java/lang/IllegalStateException"),
                jni_name(message),
            );
            None
        }
    }
}

pub(crate) fn resolve_resilience_policy_with_runtime(
    source_kind: MediaSourceKind,
    source_protocol: MediaSourceProtocol,
    buffering_policy: PlayerBufferingPolicy,
    retry_policy: PlayerRetryPolicy,
    cache_policy: PlayerCachePolicy,
) -> PlayerResolvedResiliencePolicy {
    resolve_resilience_policy_via_shared_resolver(
        source_kind,
        source_protocol,
        buffering_policy,
        retry_policy,
        cache_policy,
    )
}

pub(crate) fn resolve_track_preferences_with_runtime(
    track_preferences: PlayerTrackPreferencePolicy,
) -> PlayerTrackPreferencePolicy {
    resolve_track_preferences_via_shared_resolver(track_preferences)
}

pub(crate) fn resolve_preload_budget_with_runtime(
    preload_budget: PlayerPreloadBudgetPolicy,
) -> PlayerResolvedPreloadBudgetPolicy {
    resolve_preload_budget_via_shared_resolver(preload_budget)
}
