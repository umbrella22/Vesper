use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use jni::Env;
use jni::sys::jlong;
use player_platform_android::{AndroidHostBridgeSession, AndroidNativeFramePipelineSession};
use player_platform_mobile::MobileSourceNormalizerResourceOpen;
use player_plugin_loader::BenchmarkSinkPluginSession;
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
pub(crate) type AndroidBenchmarkSinkSession = Arc<Mutex<BenchmarkSinkPluginSession>>;

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

pub(crate) fn new_session(source_uri: String) -> Result<jlong, &'static str> {
    let session = Arc::new(Mutex::new(AndroidHostBridgeSession::new(source_uri)));
    let mut guard = lock_or_recover(sessions());
    let handle = guard.insert(session);
    if handle == 0 {
        return Err("android JNI session registry overflow");
    }
    Ok(handle)
}

pub(crate) fn new_benchmark_sink_session(paths: Vec<String>) -> Result<jlong, String> {
    let paths = paths.into_iter().map(PathBuf::from).collect::<Vec<_>>();
    let session =
        BenchmarkSinkPluginSession::load_paths(paths).map_err(|error| error.to_string())?;
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
    match f(&session) {
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
