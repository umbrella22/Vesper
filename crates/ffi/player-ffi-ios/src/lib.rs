#![warn(clippy::undocumented_unsafe_blocks)]

use std::ffi::{CStr, c_char};
use std::path::PathBuf;
use std::ptr;
use std::slice;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use player_ffi_common::{clear_c_string_output, write_c_string_output};
use player_model::MediaSource;
use player_platform_ios::{
    IosDownloadBridgeSession, IosPlaylistBridgeSession, IosPreloadBridgeSession,
};
use player_platform_mobile::{
    MobileFrameProcessorConfiguration, MobileSourceNormalizerConfiguration,
    MobileSourceNormalizerRouteDecision, mobile_plugin_diagnostics_json,
    mobile_source_normalizer_resource_bypass_diagnostics_json,
    mobile_source_normalizer_resource_open_json, mobile_source_normalizer_resource_status_json,
    open_mobile_source_normalizer_resource_with_diagnostics,
};
use player_plugin::{
    ProcessorProgress, VESPER_DECODER_PLUGIN_ABI_VERSION_CURRENT,
    VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_CURRENT,
    VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_CURRENT,
};
use player_plugin_loader::BenchmarkSinkPluginSession;
use player_runtime::{
    DownloadTaskSnapshot, FrameProcessorMode, NativeFramePipelineMode, PlayerError, PreloadBudget,
    SourceNormalizerMode,
    policy::{
        resolve_preload_budget as resolve_preload_budget_with_runtime,
        resolve_resilience_policy as resolve_resilience_policy_with_runtime,
        resolve_track_preferences as resolve_track_preferences_with_runtime,
    },
};

mod conversions;
mod handles;
mod native_frame_pipeline;
mod types;

use conversions::*;
use handles::*;
use native_frame_pipeline::{
    IosNativeFramePipelineOpenConfig, IosNativeFramePipelineSession,
    native_frame_pipeline_frame_json, native_frame_pipeline_open_json,
    native_frame_pipeline_status_json,
};
pub(crate) use types::ResolvedDownloadConfig;
pub use types::*;

#[cfg(test)]
mod tests;

/// # Safety
///
/// `out_json` must point to writable storage for a Rust-owned string pointer. The returned
/// string must be released with `player_ffi_mobile_plugin_diagnostics_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_ios_plugin_abi_summary_json(
    out_json: *mut *mut c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_json.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_json was null"),
            );
            return PlayerFfiCallStatus::Error;
        }
        clear_c_string_output(out_json);

        let summary = serde_json::json!({
            "decoderAbiVersion": VESPER_DECODER_PLUGIN_ABI_VERSION_CURRENT,
            "frameProcessorAbiVersion": VESPER_FRAME_PROCESSOR_PLUGIN_ABI_VERSION_CURRENT,
            "sourceNormalizerAbiVersion": VESPER_SOURCE_NORMALIZER_PLUGIN_ABI_VERSION_CURRENT,
            "abiSemantics": "signature-only",
            "capabilityMatching": "requirements-first",
        });
        write_c_string_output(out_json, summary.to_string());
        PlayerFfiCallStatus::Ok
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_resolve_resilience_policy(
    source_kind: u32,
    source_protocol: u32,
    buffering_policy: *const PlayerFfiBufferingPolicy,
    retry_policy: *const PlayerFfiRetryPolicy,
    cache_policy: *const PlayerFfiCachePolicy,
    out_policy: *mut PlayerFfiResolvedResiliencePolicy,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_policy.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_policy was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let buffering_policy = match read_buffering_policy(buffering_policy) {
            Ok(policy) => policy,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let retry_policy = match read_retry_policy(retry_policy) {
            Ok(policy) => policy,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let cache_policy = match read_cache_policy(cache_policy) {
            Ok(policy) => policy,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let source_kind = match media_source_kind_from_u32(source_kind) {
            Ok(kind) => kind,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let source_protocol = match media_source_protocol_from_u32(source_protocol) {
            Ok(protocol) => protocol,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let resolved = resolve_resilience_policy_with_runtime(
            source_kind,
            source_protocol,
            buffering_policy,
            retry_policy,
            cache_policy,
        );

        unsafe {
            ptr::write(out_policy, resolved.into());
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_resolve_preload_budget(
    preload_budget: *const PlayerFfiPreloadBudgetPolicy,
    out_budget: *mut PlayerFfiResolvedPreloadBudgetPolicy,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_budget.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_budget was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let preload_budget = match read_preload_budget(preload_budget) {
            Ok(preload_budget) => preload_budget,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let resolved = resolve_preload_budget_with_runtime(preload_budget);
        unsafe {
            ptr::write(out_budget, resolved.into());
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_preload_session_create(
    preload_budget: *const PlayerFfiResolvedPreloadBudgetPolicy,
    out_handle: *mut u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_handle.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_handle was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let Some(preload_budget) = (unsafe { preload_budget.as_ref() }) else {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "preload_budget was null"),
            );
            return PlayerFfiCallStatus::Error;
        };

        let budget_provider = player_runtime::InMemoryPreloadBudgetProvider::new(PreloadBudget {
            max_concurrent_tasks: preload_budget.max_concurrent_tasks,
            max_memory_bytes: preload_budget.max_memory_bytes,
            max_disk_bytes: preload_budget.max_disk_bytes,
            warmup_window: Duration::from_millis(preload_budget.warmup_window_ms),
        });
        let session = IosPreloadBridgeSession::new(budget_provider);

        let Ok(mut sessions) = preload_sessions().lock() else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "preload session registry lock failed",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };
        let handle = sessions.insert(session);
        unsafe {
            ptr::write(out_handle, handle);
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_preload_session_dispose(handle: u64) {
    ffi_void(|| {
        if let Ok(mut sessions) = preload_sessions().lock() {
            sessions.remove(handle);
        }
    });
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_preload_session_plan(
    handle: u64,
    candidates: *const PlayerFfiPreloadCandidate,
    candidates_len: usize,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        let Ok(mut sessions) = preload_sessions().lock() else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "preload session registry lock failed",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };
        let Some(session) = sessions.get_mut(handle) else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "invalid preload session handle",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };

        let candidates = if candidates_len == 0 {
            &[][..]
        } else {
            if candidates.is_null() {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::NullPointer, "candidates was null"),
                );
                return PlayerFfiCallStatus::Error;
            }
            unsafe { slice::from_raw_parts(candidates, candidates_len) }
        };

        let rust_candidates = match candidates
            .iter()
            .map(read_preload_candidate)
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(value) => value,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        session.plan(rust_candidates, std::time::Instant::now());
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_preload_session_drain_commands(
    handle: u64,
    out_commands: *mut PlayerFfiPreloadCommandList,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_commands.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_commands was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let Ok(mut sessions) = preload_sessions().lock() else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "preload session registry lock failed",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };
        let Some(session) = sessions.get_mut(handle) else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "invalid preload session handle",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };

        let commands = session
            .drain_commands()
            .into_iter()
            .map(PlayerFfiPreloadCommand::from)
            .collect::<Vec<_>>();
        let len = commands.len();
        let ptr = if len == 0 {
            ptr::null_mut()
        } else {
            Box::into_raw(commands.into_boxed_slice()) as *mut PlayerFfiPreloadCommand
        };
        unsafe {
            ptr::write(
                out_commands,
                PlayerFfiPreloadCommandList { commands: ptr, len },
            );
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_preload_session_complete(
    handle: u64,
    task_id: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        let Ok(mut sessions) = preload_sessions().lock() else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "preload session registry lock failed",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };
        let Some(session) = sessions.get_mut(handle) else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "invalid preload session handle",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };
        if let Err(error) = session.complete(player_runtime::PreloadTaskId::from_raw(task_id)) {
            write_error(out_error, player_error_to_ffi(error));
            return PlayerFfiCallStatus::Error;
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_preload_session_fail(
    handle: u64,
    task_id: u64,
    code: u32,
    category: u32,
    retriable: bool,
    message: *const c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        let message = match read_optional_c_string(message, "message") {
            Ok(Some(value)) => value,
            Ok(None) => String::new(),
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let code = match error_code_from_u32(code) {
            Ok(code) => code,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let category = match error_category_from_u32(category) {
            Ok(category) => category,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let Ok(mut sessions) = preload_sessions().lock() else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "preload session registry lock failed",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };
        let Some(session) = sessions.get_mut(handle) else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "invalid preload session handle",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };

        let error = PlayerError::with_taxonomy(code.into(), category.into(), retriable, message);
        if let Err(error) = session.fail(player_runtime::PreloadTaskId::from_raw(task_id), error) {
            write_error(out_error, player_error_to_ffi(error));
            return PlayerFfiCallStatus::Error;
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_preload_command_list_free(
    list: *mut PlayerFfiPreloadCommandList,
) {
    ffi_void(|| {
        let Some(list) = (unsafe { list.as_mut() }) else {
            return;
        };
        if !list.commands.is_null() && list.len > 0 {
            let commands = unsafe { Vec::from_raw_parts(list.commands, list.len, list.len) };
            for mut command in commands {
                preload_command_free(&mut command);
            }
        }
        *list = PlayerFfiPreloadCommandList::default();
    });
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_create(
    config: *const PlayerFfiDownloadConfig,
    out_handle: *mut u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_handle.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_handle was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let config = match read_download_config(config) {
            Ok(config) => config,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let session = match IosDownloadBridgeSession::new_with_plugin_library_paths(
            config.auto_start,
            config.run_post_processors_on_completion,
            config.plugin_library_paths,
        ) {
            Ok(session) => session,
            Err(error) => {
                write_error(out_error, player_error_to_ffi(error));
                return PlayerFfiCallStatus::Error;
            }
        };

        let Ok(mut sessions) = download_sessions().lock() else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "download session registry lock failed",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };
        let handle = sessions.insert(IosDownloadBridgeSessionHandle::new(Mutex::new(session)));
        unsafe {
            ptr::write(out_handle, handle);
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_dispose(handle: u64) {
    ffi_void(|| {
        // Remove the entry under the registry lock, but drop the returned `Arc`
        // (whose inner `IosDownloadBridgeSession::Drop` tears down the
        // `DownloadManager`, which may invoke plugin post-processor `destroy`
        // FFI) *after* releasing the registry lock. Dropping it under the lock
        // would serialize every download session process-wide.
        let session = {
            if let Ok(mut sessions) = download_sessions().lock() {
                sessions.remove(handle)
            } else {
                None
            }
        };
        drop(session);
    });
}

#[derive(Debug, Clone, Copy)]
struct FfiDownloadExportProgress {
    callbacks: PlayerFfiDownloadExportCallbacks,
}

// SAFETY: this callback table is an FFI contract provided by the host for the
// duration of a single synchronous export call.
unsafe impl Send for FfiDownloadExportProgress {}
// SAFETY: same reasoning as above; the host-provided callback context is
// expected to be safe for shared access during the export call.
unsafe impl Sync for FfiDownloadExportProgress {}

impl ProcessorProgress for FfiDownloadExportProgress {
    fn on_progress(&self, ratio: f32) {
        if let Some(on_progress) = self.callbacks.on_progress {
            unsafe { on_progress(self.callbacks.context, ratio) };
        }
    }

    fn is_cancelled(&self) -> bool {
        self.callbacks
            .is_cancelled
            .map(|is_cancelled| unsafe { is_cancelled(self.callbacks.context) })
            .unwrap_or(false)
    }
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_create_task(
    handle: u64,
    asset_id: *const c_char,
    source: *const PlayerFfiDownloadSource,
    profile: *const PlayerFfiDownloadProfile,
    asset_index: *const PlayerFfiDownloadAssetIndex,
    out_task_id: *mut u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_task_id.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_task_id was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let asset_id = match read_optional_c_string(asset_id, "asset_id") {
            Ok(Some(asset_id)) => asset_id,
            Ok(None) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::NullPointer, "asset_id was null"),
                );
                return PlayerFfiCallStatus::Error;
            }
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let source = match read_download_source(source) {
            Ok(source) => source,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let profile = match read_download_profile(profile) {
            Ok(profile) => profile,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let asset_index = match read_download_asset_index(asset_index) {
            Ok(asset_index) => asset_index,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let task_id = {
            let sessions = match download_sessions().lock() {
                Ok(sessions) => sessions,
                Err(_) => {
                    write_error(
                        out_error,
                        owned_api_error(
                            PlayerFfiErrorCode::InvalidArgument,
                            "download session registry lock failed",
                        ),
                    );
                    return PlayerFfiCallStatus::Error;
                }
            };
            let Some(session) = sessions.get(handle).cloned() else {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        "invalid download session handle",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            };
            session
        };
        let mut session = match task_id.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        let task_id = match session.create_task(
            asset_id,
            source,
            profile,
            asset_index,
            std::time::Instant::now(),
        ) {
            Ok(task_id) => task_id,
            Err(error) => {
                write_error(out_error, player_error_to_ffi(error));
                return PlayerFfiCallStatus::Error;
            }
        };
        unsafe {
            ptr::write(out_task_id, task_id.get());
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_restore_tasks(
    handle: u64,
    tasks: *const PlayerFfiDownloadTask,
    tasks_len: usize,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        let tasks = if tasks_len == 0 {
            &[][..]
        } else {
            if tasks.is_null() {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::NullPointer, "tasks was null"),
                );
                return PlayerFfiCallStatus::Error;
            }
            unsafe { slice::from_raw_parts(tasks, tasks_len) }
        };

        let now = Instant::now();
        let restored_tasks = match tasks
            .iter()
            .map(|task| read_download_task(task, now))
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(tasks) => tasks,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        if let Err(error) = {
            let sessions = match download_sessions().lock() {
                Ok(sessions) => sessions,
                Err(_) => {
                    write_error(
                        out_error,
                        owned_api_error(
                            PlayerFfiErrorCode::InvalidArgument,
                            "download session registry lock failed",
                        ),
                    );
                    return PlayerFfiCallStatus::Error;
                }
            };
            let Some(session) = sessions.get(handle).cloned() else {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        "invalid download session handle",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            };
            session
        }
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .restore_tasks(restored_tasks, now)
        {
            write_error(out_error, player_error_to_ffi(error));
            return PlayerFfiCallStatus::Error;
        }
        PlayerFfiCallStatus::Ok
    })
}

fn with_download_session_task_mutation(
    handle: u64,
    task_id: u64,
    out_error: *mut PlayerFfiError,
    mutate: impl FnOnce(
        &mut IosDownloadBridgeSession,
        player_runtime::DownloadTaskId,
        std::time::Instant,
    ) -> player_runtime::PlayerResult<Option<DownloadTaskSnapshot>>,
) -> PlayerFfiCallStatus {
    let session = {
        let sessions = match download_sessions().lock() {
            Ok(sessions) => sessions,
            Err(_) => {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        "download session registry lock failed",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        let Some(session) = sessions.get(handle).cloned() else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "invalid download session handle",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };
        session
    };
    let mut session = match session.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    if let Err(error) = mutate(
        &mut session,
        player_runtime::DownloadTaskId::from_raw(task_id),
        std::time::Instant::now(),
    ) {
        write_error(out_error, player_error_to_ffi(error));
        return PlayerFfiCallStatus::Error;
    }
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_start_task(
    handle: u64,
    task_id: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        with_download_session_task_mutation(handle, task_id, out_error, |session, task_id, now| {
            session.start_task(task_id, now)
        })
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_pause_task(
    handle: u64,
    task_id: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        with_download_session_task_mutation(handle, task_id, out_error, |session, task_id, now| {
            session.pause_task(task_id, now)
        })
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_resume_task(
    handle: u64,
    task_id: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        with_download_session_task_mutation(handle, task_id, out_error, |session, task_id, now| {
            session.resume_task(task_id, now)
        })
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_update_progress(
    handle: u64,
    task_id: u64,
    received_bytes: u64,
    received_segments: u32,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        let session = {
            let sessions = match download_sessions().lock() {
                Ok(sessions) => sessions,
                Err(_) => {
                    write_error(
                        out_error,
                        owned_api_error(
                            PlayerFfiErrorCode::InvalidArgument,
                            "download session registry lock failed",
                        ),
                    );
                    return PlayerFfiCallStatus::Error;
                }
            };
            let Some(session) = sessions.get(handle).cloned() else {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        "invalid download session handle",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            };
            session
        };
        let mut session = match session.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        if let Err(error) = session.update_progress(
            player_runtime::DownloadTaskId::from_raw(task_id),
            received_bytes,
            received_segments,
            std::time::Instant::now(),
        ) {
            write_error(out_error, player_error_to_ffi(error));
            return PlayerFfiCallStatus::Error;
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_complete_task(
    handle: u64,
    task_id: u64,
    completed_path: *const c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        let completed_path = match read_optional_c_string(completed_path, "completed_path") {
            Ok(value) => value.map(PathBuf::from),
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        // Clone the Arc under the registry lock, then drop the registry lock
        // before `complete_task`. When `run_post_processors_on_completion` is
        // true, completion synchronously invokes each plugin post-processor's
        // `process_json` FFI (FFmpeg remux/encode); holding the global registry
        // mutex across that work would serialize every download session.
        let session = {
            let sessions = match download_sessions().lock() {
                Ok(sessions) => sessions,
                Err(_) => {
                    write_error(
                        out_error,
                        owned_api_error(
                            PlayerFfiErrorCode::InvalidArgument,
                            "download session registry lock failed",
                        ),
                    );
                    return PlayerFfiCallStatus::Error;
                }
            };
            let Some(session) = sessions.get(handle).cloned() else {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        "invalid download session handle",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            };
            session
        };
        let mut session = match session.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        if let Err(error) = session.complete_task(
            player_runtime::DownloadTaskId::from_raw(task_id),
            completed_path,
            std::time::Instant::now(),
        ) {
            write_error(out_error, player_error_to_ffi(error));
            return PlayerFfiCallStatus::Error;
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_complete_preparation(
    handle: u64,
    task_id: u64,
    asset_index: *const PlayerFfiDownloadAssetIndex,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        let asset_index = match read_download_asset_index(asset_index) {
            Ok(value) => value,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let session = {
            let sessions = match download_sessions().lock() {
                Ok(sessions) => sessions,
                Err(_) => {
                    write_error(
                        out_error,
                        owned_api_error(
                            PlayerFfiErrorCode::InvalidArgument,
                            "download session registry lock failed",
                        ),
                    );
                    return PlayerFfiCallStatus::Error;
                }
            };
            let Some(session) = sessions.get(handle).cloned() else {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        "invalid download session handle",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            };
            session
        };
        let mut session = match session.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        if let Err(error) = session.complete_preparation(
            player_runtime::DownloadTaskId::from_raw(task_id),
            asset_index,
            std::time::Instant::now(),
        ) {
            write_error(out_error, player_error_to_ffi(error));
            return PlayerFfiCallStatus::Error;
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_replace_task_plan(
    handle: u64,
    task_id: u64,
    source: *const PlayerFfiDownloadSource,
    profile: *const PlayerFfiDownloadProfile,
    asset_index: *const PlayerFfiDownloadAssetIndex,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        let source = match read_download_source(source) {
            Ok(source) => source,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let profile = match read_download_profile(profile) {
            Ok(profile) => profile,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let asset_index = match read_download_asset_index(asset_index) {
            Ok(value) => value,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let session = {
            let sessions = match download_sessions().lock() {
                Ok(sessions) => sessions,
                Err(_) => {
                    write_error(
                        out_error,
                        owned_api_error(
                            PlayerFfiErrorCode::InvalidArgument,
                            "download session registry lock failed",
                        ),
                    );
                    return PlayerFfiCallStatus::Error;
                }
            };
            let Some(session) = sessions.get(handle).cloned() else {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        "invalid download session handle",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            };
            session
        };
        let mut session = match session.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        if let Err(error) = session.replace_task_plan(
            player_runtime::DownloadTaskId::from_raw(task_id),
            source,
            profile,
            asset_index,
            std::time::Instant::now(),
        ) {
            write_error(out_error, player_error_to_ffi(error));
            return PlayerFfiCallStatus::Error;
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_export_task(
    handle: u64,
    task_id: u64,
    output_path: *const c_char,
    callbacks: PlayerFfiDownloadExportCallbacks,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        let output_path = match read_optional_c_string(output_path, "output_path") {
            Ok(Some(path)) => path,
            Ok(None) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::NullPointer, "output_path was null"),
                );
                return PlayerFfiCallStatus::Error;
            }
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let progress = FfiDownloadExportProgress { callbacks };
        // Clone the Arc under the registry lock, then drop the registry lock
        // before `export_task_output`. Export runs the plugin post-processor
        // chain (`process_json` FFI) and the host progress callback; holding the
        // global registry mutex across that work would serialize every download
        // session.
        let session = {
            let sessions = match download_sessions().lock() {
                Ok(sessions) => sessions,
                Err(_) => {
                    write_error(
                        out_error,
                        owned_api_error(
                            PlayerFfiErrorCode::InvalidArgument,
                            "download session registry lock failed",
                        ),
                    );
                    return PlayerFfiCallStatus::Error;
                }
            };
            let Some(session) = sessions.get(handle).cloned() else {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        "invalid download session handle",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            };
            session
        };
        let session = match session.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        if let Err(error) = session.export_task_output(
            player_runtime::DownloadTaskId::from_raw(task_id),
            Some(PathBuf::from(output_path)),
            &progress,
        ) {
            write_error(out_error, player_error_to_ffi(error));
            return PlayerFfiCallStatus::Error;
        }

        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_fail_task(
    handle: u64,
    task_id: u64,
    code: u32,
    category: u32,
    retriable: bool,
    message: *const c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        let message = match read_optional_c_string(message, "message") {
            Ok(Some(value)) => value,
            Ok(None) => String::new(),
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let code = match error_code_from_u32(code) {
            Ok(code) => code,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let category = match error_category_from_u32(category) {
            Ok(category) => category,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let session = {
            let sessions = match download_sessions().lock() {
                Ok(sessions) => sessions,
                Err(_) => {
                    write_error(
                        out_error,
                        owned_api_error(
                            PlayerFfiErrorCode::InvalidArgument,
                            "download session registry lock failed",
                        ),
                    );
                    return PlayerFfiCallStatus::Error;
                }
            };
            let Some(session) = sessions.get(handle).cloned() else {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        "invalid download session handle",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            };
            session
        };
        let mut session = match session.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        let error = PlayerError::with_taxonomy(code.into(), category.into(), retriable, message);
        if let Err(error) = session.fail_task(
            player_runtime::DownloadTaskId::from_raw(task_id),
            error,
            std::time::Instant::now(),
        ) {
            write_error(out_error, player_error_to_ffi(error));
            return PlayerFfiCallStatus::Error;
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_remove_task(
    handle: u64,
    task_id: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        with_download_session_task_mutation(handle, task_id, out_error, |session, task_id, now| {
            session.remove_task(task_id, now)
        })
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_snapshot(
    handle: u64,
    out_snapshot: *mut PlayerFfiDownloadSnapshot,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_snapshot.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_snapshot was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let session = {
            let sessions = match download_sessions().lock() {
                Ok(sessions) => sessions,
                Err(_) => {
                    write_error(
                        out_error,
                        owned_api_error(
                            PlayerFfiErrorCode::InvalidArgument,
                            "download session registry lock failed",
                        ),
                    );
                    return PlayerFfiCallStatus::Error;
                }
            };
            let Some(session) = sessions.get(handle).cloned() else {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        "invalid download session handle",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            };
            session
        };
        let session = match session.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        let tasks = session
            .snapshot()
            .tasks
            .into_iter()
            .map(download_task_to_ffi)
            .collect::<Vec<_>>();
        let len = tasks.len();
        let ptr = if len == 0 {
            ptr::null_mut()
        } else {
            Box::into_raw(tasks.into_boxed_slice()) as *mut PlayerFfiDownloadTask
        };
        unsafe {
            ptr::write(out_snapshot, PlayerFfiDownloadSnapshot { tasks: ptr, len });
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_drain_commands(
    handle: u64,
    out_commands: *mut PlayerFfiDownloadCommandList,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_commands.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_commands was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let session = {
            let sessions = match download_sessions().lock() {
                Ok(sessions) => sessions,
                Err(_) => {
                    write_error(
                        out_error,
                        owned_api_error(
                            PlayerFfiErrorCode::InvalidArgument,
                            "download session registry lock failed",
                        ),
                    );
                    return PlayerFfiCallStatus::Error;
                }
            };
            let Some(session) = sessions.get(handle).cloned() else {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        "invalid download session handle",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            };
            session
        };
        let mut session = match session.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        let commands = session
            .drain_commands()
            .into_iter()
            .map(PlayerFfiDownloadCommand::from)
            .collect::<Vec<_>>();
        let len = commands.len();
        let ptr = if len == 0 {
            ptr::null_mut()
        } else {
            Box::into_raw(commands.into_boxed_slice()) as *mut PlayerFfiDownloadCommand
        };
        unsafe {
            ptr::write(
                out_commands,
                PlayerFfiDownloadCommandList { commands: ptr, len },
            );
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_session_drain_events(
    handle: u64,
    out_events: *mut PlayerFfiDownloadEventList,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_events.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_events was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let session = {
            let sessions = match download_sessions().lock() {
                Ok(sessions) => sessions,
                Err(_) => {
                    write_error(
                        out_error,
                        owned_api_error(
                            PlayerFfiErrorCode::InvalidArgument,
                            "download session registry lock failed",
                        ),
                    );
                    return PlayerFfiCallStatus::Error;
                }
            };
            let Some(session) = sessions.get(handle).cloned() else {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        "invalid download session handle",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            };
            session
        };
        let mut session = match session.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        let events = session
            .drain_events()
            .into_iter()
            .map(PlayerFfiDownloadEvent::from)
            .collect::<Vec<_>>();
        let len = events.len();
        let ptr = if len == 0 {
            ptr::null_mut()
        } else {
            Box::into_raw(events.into_boxed_slice()) as *mut PlayerFfiDownloadEvent
        };
        unsafe {
            ptr::write(out_events, PlayerFfiDownloadEventList { events: ptr, len });
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_snapshot_free(
    snapshot: *mut PlayerFfiDownloadSnapshot,
) {
    ffi_void(|| {
        let Some(snapshot) = (unsafe { snapshot.as_mut() }) else {
            return;
        };
        if !snapshot.tasks.is_null() && snapshot.len > 0 {
            let tasks = unsafe { Vec::from_raw_parts(snapshot.tasks, snapshot.len, snapshot.len) };
            for mut task in tasks {
                download_task_free(&mut task);
            }
        }
        *snapshot = PlayerFfiDownloadSnapshot::default();
    });
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_command_list_free(
    list: *mut PlayerFfiDownloadCommandList,
) {
    ffi_void(|| {
        let Some(list) = (unsafe { list.as_mut() }) else {
            return;
        };
        if !list.commands.is_null() && list.len > 0 {
            let commands = unsafe { Vec::from_raw_parts(list.commands, list.len, list.len) };
            for mut command in commands {
                download_command_free(&mut command);
            }
        }
        *list = PlayerFfiDownloadCommandList::default();
    });
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_download_event_list_free(
    list: *mut PlayerFfiDownloadEventList,
) {
    ffi_void(|| {
        let Some(list) = (unsafe { list.as_mut() }) else {
            return;
        };
        if !list.events.is_null() && list.len > 0 {
            let events = unsafe { Vec::from_raw_parts(list.events, list.len, list.len) };
            for mut event in events {
                download_event_free(&mut event);
            }
        }
        *list = PlayerFfiDownloadEventList::default();
    });
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_create(
    config: *const PlayerFfiPlaylistConfig,
    preload_budget: *const PlayerFfiResolvedPreloadBudgetPolicy,
    out_handle: *mut u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_handle.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_handle was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let config = match read_playlist_config(config) {
            Ok(config) => config,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let Some(preload_budget) = (unsafe { preload_budget.as_ref() }) else {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "preload_budget was null"),
            );
            return PlayerFfiCallStatus::Error;
        };

        let session = IosPlaylistBridgeSession::new(
            config.0,
            config.1,
            PreloadBudget {
                max_concurrent_tasks: preload_budget.max_concurrent_tasks,
                max_memory_bytes: preload_budget.max_memory_bytes,
                max_disk_bytes: preload_budget.max_disk_bytes,
                warmup_window: Duration::from_millis(preload_budget.warmup_window_ms),
            },
        );

        let Ok(mut sessions) = playlist_sessions().lock() else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "playlist session registry lock failed",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };
        let handle = sessions.insert(session);
        unsafe {
            ptr::write(out_handle, handle);
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_dispose(handle: u64) {
    ffi_void(|| {
        if let Ok(mut sessions) = playlist_sessions().lock() {
            sessions.remove(handle);
        }
    });
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_replace_queue(
    handle: u64,
    queue: *const PlayerFfiPlaylistQueueItem,
    queue_len: usize,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        let Ok(mut sessions) = playlist_sessions().lock() else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "playlist session registry lock failed",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };
        let Some(session) = sessions.get_mut(handle) else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "invalid playlist session handle",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };

        let queue = if queue_len == 0 {
            &[][..]
        } else {
            if queue.is_null() {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::NullPointer, "queue was null"),
                );
                return PlayerFfiCallStatus::Error;
            }
            unsafe { slice::from_raw_parts(queue, queue_len) }
        };

        let rust_queue = match queue
            .iter()
            .map(read_playlist_queue_item)
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(value) => value,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        session.replace_queue(rust_queue, std::time::Instant::now());
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_update_viewport_hints(
    handle: u64,
    hints: *const PlayerFfiPlaylistViewportHint,
    hints_len: usize,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        let Ok(mut sessions) = playlist_sessions().lock() else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "playlist session registry lock failed",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };
        let Some(session) = sessions.get_mut(handle) else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "invalid playlist session handle",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };

        let hints = if hints_len == 0 {
            &[][..]
        } else {
            if hints.is_null() {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::NullPointer, "hints was null"),
                );
                return PlayerFfiCallStatus::Error;
            }
            unsafe { slice::from_raw_parts(hints, hints_len) }
        };

        let rust_hints = match hints
            .iter()
            .map(read_playlist_viewport_hint)
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(value) => value,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        session.update_viewport_hints(rust_hints, std::time::Instant::now());
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_clear_viewport_hints(
    handle: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        let Ok(mut sessions) = playlist_sessions().lock() else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "playlist session registry lock failed",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };
        let Some(session) = sessions.get_mut(handle) else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "invalid playlist session handle",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };

        session.clear_viewport_hints(std::time::Instant::now());
        PlayerFfiCallStatus::Ok
    })
}

fn with_playlist_session_advance(
    handle: u64,
    out_error: *mut PlayerFfiError,
    advance: impl FnOnce(&mut IosPlaylistBridgeSession, std::time::Instant),
) -> PlayerFfiCallStatus {
    let Ok(mut sessions) = playlist_sessions().lock() else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "playlist session registry lock failed",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };
    let Some(session) = sessions.get_mut(handle) else {
        write_error(
            out_error,
            owned_api_error(
                PlayerFfiErrorCode::InvalidArgument,
                "invalid playlist session handle",
            ),
        );
        return PlayerFfiCallStatus::Error;
    };

    advance(session, std::time::Instant::now());
    PlayerFfiCallStatus::Ok
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_advance_to_next(
    handle: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        with_playlist_session_advance(handle, out_error, |session, now| {
            let _ = session.advance_to_next(now);
        })
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_advance_to_previous(
    handle: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        with_playlist_session_advance(handle, out_error, |session, now| {
            let _ = session.advance_to_previous(now);
        })
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_handle_playback_completed(
    handle: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        with_playlist_session_advance(handle, out_error, |session, now| {
            let _ = session.handle_playback_completed(now);
        })
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_handle_playback_failed(
    handle: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        with_playlist_session_advance(handle, out_error, |session, now| {
            let _ = session.handle_playback_failed(now);
        })
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_current_active_item(
    handle: u64,
    out_active_item: *mut PlayerFfiPlaylistActiveItem,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_active_item.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_active_item was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let Ok(sessions) = playlist_sessions().lock() else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "playlist session registry lock failed",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };
        let Some(session) = sessions.get(handle) else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "invalid playlist session handle",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };

        let active_item = session
            .active_item()
            .map(playlist_active_item_to_ffi)
            .unwrap_or_default();
        unsafe {
            ptr::write(out_active_item, active_item);
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_active_item_free(
    item: *mut PlayerFfiPlaylistActiveItem,
) {
    ffi_void(|| {
        let Some(item) = (unsafe { item.as_mut() }) else {
            return;
        };
        free_c_string(&mut item.item_id);
        *item = PlayerFfiPlaylistActiveItem::default();
    });
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_drain_preload_commands(
    handle: u64,
    out_commands: *mut PlayerFfiPreloadCommandList,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_commands.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_commands was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let Ok(mut sessions) = playlist_sessions().lock() else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "playlist session registry lock failed",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };
        let Some(session) = sessions.get_mut(handle) else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "invalid playlist session handle",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };

        let commands = session
            .drain_commands()
            .into_iter()
            .map(PlayerFfiPreloadCommand::from)
            .collect::<Vec<_>>();
        let len = commands.len();
        let ptr = if len == 0 {
            ptr::null_mut()
        } else {
            Box::into_raw(commands.into_boxed_slice()) as *mut PlayerFfiPreloadCommand
        };
        unsafe {
            ptr::write(
                out_commands,
                PlayerFfiPreloadCommandList { commands: ptr, len },
            );
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_complete_preload_task(
    handle: u64,
    task_id: u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        let Ok(mut sessions) = playlist_sessions().lock() else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "playlist session registry lock failed",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };
        let Some(session) = sessions.get_mut(handle) else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "invalid playlist session handle",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };

        if let Err(error) =
            session.complete_preload_task(player_runtime::PreloadTaskId::from_raw(task_id))
        {
            write_error(out_error, player_error_to_ffi(error));
            return PlayerFfiCallStatus::Error;
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_session_fail_preload_task(
    handle: u64,
    task_id: u64,
    code: u32,
    category: u32,
    retriable: bool,
    message: *const c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        let message = match read_optional_c_string(message, "message") {
            Ok(Some(value)) => value,
            Ok(None) => String::new(),
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let code = match error_code_from_u32(code) {
            Ok(code) => code,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let category = match error_category_from_u32(category) {
            Ok(category) => category,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let Ok(mut sessions) = playlist_sessions().lock() else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "playlist session registry lock failed",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };
        let Some(session) = sessions.get_mut(handle) else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "invalid playlist session handle",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };

        let error = PlayerError::with_taxonomy(code.into(), category.into(), retriable, message);
        if let Err(error) =
            session.fail_preload_task(player_runtime::PreloadTaskId::from_raw(task_id), error)
        {
            write_error(out_error, player_error_to_ffi(error));
            return PlayerFfiCallStatus::Error;
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_resolve_track_preferences(
    track_preferences: *const PlayerFfiTrackPreferences,
    out_preferences: *mut PlayerFfiTrackPreferences,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_preferences.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_preferences was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let track_preferences = match read_track_preferences(track_preferences) {
            Ok(track_preferences) => track_preferences,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let resolved = resolve_track_preferences_with_runtime(track_preferences);
        unsafe {
            ptr::write(out_preferences, resolved.into());
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_benchmark_session_create(
    plugin_library_paths: *mut *mut c_char,
    plugin_library_paths_len: usize,
    out_handle: *mut u64,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_handle.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_handle was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let plugin_library_paths = match read_string_list(
            plugin_library_paths,
            plugin_library_paths_len,
            "plugin_library_paths",
        ) {
            Ok(paths) => paths.into_iter().map(PathBuf::from).collect::<Vec<_>>(),
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };

        let session = match BenchmarkSinkPluginSession::load_paths(plugin_library_paths) {
            Ok(session) => session,
            Err(error) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::InvalidArgument, &error.to_string()),
                );
                return PlayerFfiCallStatus::Error;
            }
        };

        let Ok(mut sessions) = benchmark_sessions().lock() else {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::InvalidArgument,
                    "benchmark session registry lock failed",
                ),
            );
            return PlayerFfiCallStatus::Error;
        };
        let handle = sessions.insert(IosBenchmarkSinkSession::new(Mutex::new(session)));
        unsafe {
            ptr::write(out_handle, handle);
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_benchmark_session_dispose(handle: u64) {
    ffi_void(|| {
        // Remove the entry under the registry lock, but drop the returned
        // `Arc` (and therefore the inner `BenchmarkSinkPluginSession`, whose
        // `Drop` invokes plugin FFI `destroy`) *after* releasing the registry
        // lock. Dropping it under the lock would serialize every benchmark
        // session process-wide and block create/dispose for the plugin call.
        let session = {
            if let Ok(mut sessions) = benchmark_sessions().lock() {
                sessions.remove(handle)
            } else {
                None
            }
        };
        drop(session);
    });
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_benchmark_session_on_event_batch_json(
    handle: u64,
    batch_json: *const c_char,
    out_report_json: *mut *mut c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if batch_json.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "batch_json was null"),
            );
            return PlayerFfiCallStatus::Error;
        }
        if out_report_json.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_report_json was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let batch_json = match unsafe { CStr::from_ptr(batch_json) }.to_str() {
            Ok(value) => value,
            Err(_) => {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidUtf8,
                        "batch_json was not valid UTF-8",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            }
        };

        // Clone the Arc under the registry lock, then drop the registry lock
        // before invoking the plugin FFI call. `on_event_batch_report_json`
        // crosses into a dlopen-loaded plugin; holding the global registry
        // mutex across that call would serialize every benchmark session
        // process-wide and block create/dispose operations.
        let session = {
            let Ok(sessions) = benchmark_sessions().lock() else {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        "benchmark session registry lock failed",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            };
            let Some(session) = sessions.get(handle).cloned() else {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        "invalid benchmark session handle",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            };
            session
        };
        let session = match session.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        let report_json = match session.on_event_batch_report_json(batch_json) {
            Ok(value) => value,
            Err(error) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::InvalidArgument, &error.to_string()),
                );
                return PlayerFfiCallStatus::Error;
            }
        };

        write_c_string_output(out_report_json, report_json);
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_benchmark_session_flush_json(
    handle: u64,
    out_report_json: *mut *mut c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_report_json.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_report_json was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        // Clone the Arc under the registry lock, then drop the registry lock
        // before invoking the plugin FFI call. `flush_json` crosses into a
        // dlopen-loaded plugin; holding the global registry mutex across that
        // call would serialize every benchmark session process-wide.
        let session = {
            let Ok(sessions) = benchmark_sessions().lock() else {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        "benchmark session registry lock failed",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            };
            let Some(session) = sessions.get(handle).cloned() else {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        "invalid benchmark session handle",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            };
            session
        };
        let session = match session.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        let report_json = match session.flush_json() {
            Ok(value) => value,
            Err(error) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::InvalidArgument, &error.to_string()),
                );
                return PlayerFfiCallStatus::Error;
            }
        };

        write_c_string_output(out_report_json, report_json);
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_benchmark_report_string_free(value: *mut c_char) {
    ffi_void(|| {
        let mut value = value;
        free_c_string(&mut value);
    });
}

/// # Safety
///
/// String and array pointers must be valid for the duration of the call. The returned JSON string
/// is allocated by Rust and must be released with `player_ffi_mobile_plugin_diagnostics_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_mobile_plugin_diagnostics_json(
    source_uri: *const c_char,
    source_mode: u32,
    source_plugin_library_paths: *mut *mut c_char,
    source_plugin_library_paths_len: usize,
    runtime_profile: *const c_char,
    frame_mode: u32,
    frame_plugin_library_paths: *mut *mut c_char,
    frame_plugin_library_paths_len: usize,
    out_json: *mut *mut c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_json.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_json was null"),
            );
            return PlayerFfiCallStatus::Error;
        }
        clear_c_string_output(out_json);

        let source_uri = match read_optional_c_string(source_uri, "source_uri") {
            Ok(Some(value)) => value,
            Ok(None) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::NullPointer, "source_uri was null"),
                );
                return PlayerFfiCallStatus::Error;
            }
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let source_plugin_library_paths = match read_string_list(
            source_plugin_library_paths,
            source_plugin_library_paths_len,
            "source_plugin_library_paths",
        ) {
            Ok(value) => value.into_iter().map(PathBuf::from).collect(),
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let frame_plugin_library_paths = match read_string_list(
            frame_plugin_library_paths,
            frame_plugin_library_paths_len,
            "frame_plugin_library_paths",
        ) {
            Ok(value) => value.into_iter().map(PathBuf::from).collect(),
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let runtime_profile = match read_optional_c_string(runtime_profile, "runtime_profile") {
            Ok(value) => value,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let diagnostics_json = match mobile_plugin_diagnostics_json(
            &MediaSource::new(source_uri),
            &MobileSourceNormalizerConfiguration {
                mode: source_normalizer_mode_from_u32(source_mode),
                plugin_library_paths: source_plugin_library_paths,
                runtime_profile,
            },
            &MobileFrameProcessorConfiguration {
                mode: frame_processor_mode_from_u32(frame_mode),
                plugin_library_paths: frame_plugin_library_paths,
            },
        ) {
            Ok(value) => value,
            Err(error) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::BackendFailure, &error.to_string()),
                );
                return PlayerFfiCallStatus::Error;
            }
        };

        write_c_string_output(out_json, diagnostics_json);
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// `value` must either be null or a Rust-owned string returned by
/// `player_ffi_mobile_plugin_diagnostics_json`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_mobile_plugin_diagnostics_string_free(value: *mut c_char) {
    ffi_void(|| {
        let mut value = value;
        free_c_string(&mut value);
    });
}

/// # Safety
///
/// String and array pointers must be valid for the duration of the call. The returned JSON string
/// is allocated by Rust and must be released with
/// `player_ffi_mobile_plugin_diagnostics_string_free`. A zero handle means no resource session was
/// opened; `out_json` may still contain bypass diagnostics in that case. A non-zero returned handle
/// must be disposed with `player_ffi_source_normalizer_resource_dispose`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_source_normalizer_resource_open(
    source_uri: *const c_char,
    source_mode: u32,
    source_plugin_library_paths: *mut *mut c_char,
    source_plugin_library_paths_len: usize,
    runtime_profile: *const c_char,
    output_root: *const c_char,
    force_normalized: bool,
    out_handle: *mut u64,
    out_json: *mut *mut c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_handle.is_null() || out_json.is_null() {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::NullPointer,
                    "out_handle or out_json was null",
                ),
            );
            return PlayerFfiCallStatus::Error;
        }
        unsafe {
            ptr::write(out_handle, 0);
            ptr::write(out_json, ptr::null_mut());
        }

        let source_uri = match read_optional_c_string(source_uri, "source_uri") {
            Ok(Some(value)) => value,
            Ok(None) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::NullPointer, "source_uri was null"),
                );
                return PlayerFfiCallStatus::Error;
            }
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let output_root = match read_optional_c_string(output_root, "output_root") {
            Ok(Some(value)) => value,
            Ok(None) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::NullPointer, "output_root was null"),
                );
                return PlayerFfiCallStatus::Error;
            }
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let plugin_library_paths = match read_string_list(
            source_plugin_library_paths,
            source_plugin_library_paths_len,
            "source_plugin_library_paths",
        ) {
            Ok(value) => value.into_iter().map(PathBuf::from).collect(),
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let runtime_profile = match read_optional_c_string(runtime_profile, "runtime_profile") {
            Ok(value) => value,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let decision = if force_normalized {
            MobileSourceNormalizerRouteDecision::Force
        } else {
            MobileSourceNormalizerRouteDecision::NativeFirst
        };
        let outcome = match open_mobile_source_normalizer_resource_with_diagnostics(
            &MediaSource::new(source_uri),
            &MobileSourceNormalizerConfiguration {
                mode: source_normalizer_mode_from_u32(source_mode),
                plugin_library_paths,
                runtime_profile,
            },
            output_root,
            decision,
        ) {
            Ok(outcome) => outcome,
            Err(error) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::BackendFailure, &error),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        let opened = match outcome.opened {
            Some(opened) => opened,
            None => {
                if !outcome.diagnostics.is_empty() {
                    let json = match mobile_source_normalizer_resource_bypass_diagnostics_json(
                        &outcome.diagnostics,
                    ) {
                        Ok(value) => value,
                        Err(error) => {
                            write_error(
                                out_error,
                                owned_api_error(
                                    PlayerFfiErrorCode::BackendFailure,
                                    &error.to_string(),
                                ),
                            );
                            return PlayerFfiCallStatus::Error;
                        }
                    };
                    unsafe {
                        ptr::write(out_json, into_c_string_ptr(json));
                    }
                }
                return PlayerFfiCallStatus::Ok;
            }
        };
        let mut sessions = match source_normalizer_resource_sessions().lock() {
            Ok(sessions) => sessions,
            Err(_) => {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidState,
                        "source normalizer resource registry lock failed",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        let handle = sessions.insert(IosSourceNormalizerResourceSession::new(Mutex::new(opened)));
        let opened = match sessions.get(handle).cloned() {
            Some(opened) => opened,
            None => {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidState,
                        "source normalizer resource registry insert failed",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        // Drop the registry lock before any potentially blocking work; opening
        // already performed its I/O, but JSON serialization may also walk the
        // resource list and we want to keep the registry lock short.
        drop(sessions);
        let opened_guard = match opened.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let json = match mobile_source_normalizer_resource_open_json(
            handle,
            &opened_guard,
            None,
        ) {
            Ok(value) => value,
            Err(error) => {
                drop(opened_guard);
                if let Ok(mut sessions) = source_normalizer_resource_sessions().lock() {
                    let _ = sessions.remove(handle);
                }
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::BackendFailure, &error.to_string()),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        unsafe {
            ptr::write(out_handle, handle);
            ptr::write(out_json, into_c_string_ptr(json));
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// The handle must have been returned by `player_ffi_source_normalizer_resource_open`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_source_normalizer_resource_poll(
    handle: u64,
    out_json: *mut *mut c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_json.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_json was null"),
            );
            return PlayerFfiCallStatus::Error;
        }
        unsafe {
            ptr::write(out_json, ptr::null_mut());
        }
        let session = {
            let sessions = match source_normalizer_resource_sessions().lock() {
                Ok(sessions) => sessions,
                Err(_) => {
                    write_error(
                        out_error,
                        owned_api_error(
                            PlayerFfiErrorCode::InvalidState,
                            "source normalizer resource registry lock failed",
                        ),
                    );
                    return PlayerFfiCallStatus::Error;
                }
            };
            let Some(session) = sessions.get(handle).cloned() else {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        "invalid source normalizer resource handle",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            };
            session
        };
        // The registry lock is released above. Plugin `poll()` performs blocking
        // filesystem I/O (directory walks, disk-usage scans); running it under
        // the global registry lock would serialize every source-normalizer
        // session process-wide and block open/dispose calls.
        let mut opened = match session.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let status = match opened.session.poll() {
            Ok(status) => status,
            Err(error) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::BackendFailure, &error.to_string()),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        opened.status = status;
        let json = match mobile_source_normalizer_resource_status_json(handle, &opened, None) {
            Ok(value) => value,
            Err(error) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::BackendFailure, &error.to_string()),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        unsafe {
            ptr::write(out_json, into_c_string_ptr(json));
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// The handle must have been returned by `player_ffi_source_normalizer_resource_open`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_source_normalizer_resource_dispose(handle: u64) {
    ffi_void(|| {
        // Remove the entry under the registry lock, but drop the returned `Arc`
        // (whose inner `MobileSourceNormalizerResourceOpen::Drop` calls plugin
        // FFI `close_resource_session`) *after* releasing the registry lock.
        let session = {
            if let Ok(mut sessions) = source_normalizer_resource_sessions().lock() {
                sessions.remove(handle)
            } else {
                None
            }
        };
        drop(session);
    });
}

/// # Safety
///
/// String and array pointers must be valid for the duration of the call. The returned JSON string
/// is allocated by Rust and must be released with
/// `player_ffi_mobile_plugin_diagnostics_string_free`. The returned handle must be disposed with
/// `player_ffi_ios_native_frame_pipeline_close`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_ios_native_frame_pipeline_open(
    source_uri: *const c_char,
    source_mode: u32,
    source_plugin_library_paths: *mut *mut c_char,
    source_plugin_library_paths_len: usize,
    runtime_profile: *const c_char,
    native_frame_mode: u32,
    decoder_plugin_library_paths: *mut *mut c_char,
    decoder_plugin_library_paths_len: usize,
    frame_plugin_library_paths: *mut *mut c_char,
    frame_plugin_library_paths_len: usize,
    max_in_flight_frames: u32,
    out_handle: *mut u64,
    out_json: *mut *mut c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_handle.is_null() || out_json.is_null() {
            write_error(
                out_error,
                owned_api_error(
                    PlayerFfiErrorCode::NullPointer,
                    "out_handle or out_json was null",
                ),
            );
            return PlayerFfiCallStatus::Error;
        }
        unsafe {
            ptr::write(out_handle, 0);
            ptr::write(out_json, ptr::null_mut());
        }
        let source_uri = match read_optional_c_string(source_uri, "source_uri") {
            Ok(Some(value)) => value,
            Ok(None) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::NullPointer, "source_uri was null"),
                );
                return PlayerFfiCallStatus::Error;
            }
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let source_plugin_library_paths = match read_string_list(
            source_plugin_library_paths,
            source_plugin_library_paths_len,
            "source_plugin_library_paths",
        ) {
            Ok(value) => value.into_iter().map(PathBuf::from).collect(),
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let decoder_plugin_library_paths = match read_string_list(
            decoder_plugin_library_paths,
            decoder_plugin_library_paths_len,
            "decoder_plugin_library_paths",
        ) {
            Ok(value) => value.into_iter().map(PathBuf::from).collect(),
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let frame_plugin_library_paths = match read_string_list(
            frame_plugin_library_paths,
            frame_plugin_library_paths_len,
            "frame_plugin_library_paths",
        ) {
            Ok(value) => value.into_iter().map(PathBuf::from).collect(),
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let runtime_profile = match read_optional_c_string(runtime_profile, "runtime_profile") {
            Ok(value) => value,
            Err(error) => {
                write_error(out_error, error);
                return PlayerFfiCallStatus::Error;
            }
        };
        let session = match IosNativeFramePipelineSession::open(IosNativeFramePipelineOpenConfig {
            source_uri,
            source_normalizer_mode: source_normalizer_mode_from_u32(source_mode),
            source_normalizer_plugin_library_paths: source_plugin_library_paths,
            runtime_profile,
            native_frame_pipeline_mode: native_frame_pipeline_mode_from_u32(native_frame_mode),
            decoder_plugin_library_paths,
            frame_processor_plugin_library_paths: frame_plugin_library_paths,
            max_in_flight_frames: (max_in_flight_frames > 0).then_some(max_in_flight_frames),
        }) {
            Ok(session) => session,
            Err(error) => {
                let message = error.wire_message();
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::BackendFailure, &message),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        let mut sessions = match native_frame_pipeline_sessions().lock() {
            Ok(sessions) => sessions,
            Err(_) => {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidState,
                        "native-frame pipeline registry lock failed",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        let handle = sessions.insert(IosNativeFramePipelineSessionHandle::new(Mutex::new(session)));
        let opened = match sessions.get(handle).cloned() {
            Some(opened) => opened,
            None => {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidState,
                        "native-frame pipeline session handle could not be resolved",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        // Drop the registry lock before JSON serialization; the open work
        // (decoders/plugins) already ran above and the registry lock should not
        // be held across any potentially blocking call.
        drop(sessions);
        let opened = match opened.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let json = match native_frame_pipeline_open_json(handle, &opened) {
            Ok(value) => value,
            Err(error) => {
                drop(opened);
                if let Ok(mut sessions) = native_frame_pipeline_sessions().lock() {
                    let _ = sessions.remove(handle);
                }
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::BackendFailure, &error.to_string()),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        unsafe {
            ptr::write(out_handle, handle);
            ptr::write(out_json, into_c_string_ptr(json));
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// The handle must have been returned by `player_ffi_ios_native_frame_pipeline_open`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_ios_native_frame_pipeline_advance(
    handle: u64,
    out_json: *mut *mut c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_json.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_json was null"),
            );
            return PlayerFfiCallStatus::Error;
        }
        unsafe {
            ptr::write(out_json, ptr::null_mut());
        }
        // Clone the Arc under the registry lock, then drop the registry lock
        // before invoking `advance()`. `advance()` performs blocking
        // VideoToolbox decode, plugin FFI packet sends, and an EOS-drain sleep;
        // holding the global registry mutex across that work would serialize
        // every native-frame pipeline session process-wide and block open/close.
        let session = {
            let sessions = match native_frame_pipeline_sessions().lock() {
                Ok(sessions) => sessions,
                Err(_) => {
                    write_error(
                        out_error,
                        owned_api_error(
                            PlayerFfiErrorCode::InvalidState,
                            "native-frame pipeline registry lock failed",
                        ),
                    );
                    return PlayerFfiCallStatus::Error;
                }
            };
            let Some(session) = sessions.get(handle).cloned() else {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        "invalid native-frame pipeline handle",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            };
            session
        };
        let mut session = match session.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let frame = match session.advance() {
            Ok(frame) => frame,
            Err(error) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::BackendFailure, &error),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        let (frame_handle, json) = match frame {
            Some(frame) => {
                let frame_handle = session.store_frame(frame);
                let Some(stored) = session.pending_frame(frame_handle) else {
                    write_error(
                        out_error,
                        owned_api_error(
                            PlayerFfiErrorCode::InvalidState,
                            "native-frame pending frame could not be stored",
                        ),
                    );
                    return PlayerFfiCallStatus::Error;
                };
                (
                    Some(frame_handle),
                    native_frame_pipeline_frame_json(
                        Some(frame_handle),
                        Some(stored),
                        session.status_wire(handle, None).counters,
                        false,
                        None,
                    ),
                )
            }
            None => {
                let end_of_stream = session.is_end_of_stream();
                (
                    None,
                    native_frame_pipeline_frame_json(
                        None,
                        None,
                        session.status_wire(handle, None).counters,
                        end_of_stream,
                        None,
                    ),
                )
            }
        };
        let json = match json {
            Ok(value) => value,
            Err(error) => {
                if let Some(frame_handle) = frame_handle {
                    let _ = session.release_pending_frame(frame_handle, false);
                }
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::BackendFailure, &error.to_string()),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        unsafe {
            ptr::write(out_json, into_c_string_ptr(json));
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// The handle must have been returned by `player_ffi_ios_native_frame_pipeline_open`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_ios_native_frame_pipeline_release_frame(
    handle: u64,
    frame_handle: u64,
    presented: bool,
    out_json: *mut *mut c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_json.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_json was null"),
            );
            return PlayerFfiCallStatus::Error;
        }
        unsafe {
            ptr::write(out_json, ptr::null_mut());
        }
        // Clone the Arc under the registry lock, then drop the registry lock
        // before `release_pending_frame` (which may invoke plugin FFI release).
        let session = {
            let sessions = match native_frame_pipeline_sessions().lock() {
                Ok(sessions) => sessions,
                Err(_) => {
                    write_error(
                        out_error,
                        owned_api_error(
                            PlayerFfiErrorCode::InvalidState,
                            "native-frame pipeline registry lock failed",
                        ),
                    );
                    return PlayerFfiCallStatus::Error;
                }
            };
            let Some(session) = sessions.get(handle).cloned() else {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        "invalid native-frame pipeline handle",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            };
            session
        };
        let mut session = match session.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Err(error) = session.release_pending_frame(frame_handle, presented) {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::BackendFailure, &error),
            );
            return PlayerFfiCallStatus::Error;
        }
        let json = match native_frame_pipeline_status_json(handle, &*session, None) {
            Ok(value) => value,
            Err(error) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::BackendFailure, &error.to_string()),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        unsafe {
            ptr::write(out_json, into_c_string_ptr(json));
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// The handle must have been returned by `player_ffi_ios_native_frame_pipeline_open`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_ios_native_frame_pipeline_flush(
    handle: u64,
    out_json: *mut *mut c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_json.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_json was null"),
            );
            return PlayerFfiCallStatus::Error;
        }
        unsafe {
            ptr::write(out_json, ptr::null_mut());
        }
        // Clone the Arc under the registry lock, then drop the registry lock
        // before `flush()` (which flushes decoder/packet/processor plugin FFI).
        let session = {
            let sessions = match native_frame_pipeline_sessions().lock() {
                Ok(sessions) => sessions,
                Err(_) => {
                    write_error(
                        out_error,
                        owned_api_error(
                            PlayerFfiErrorCode::InvalidState,
                            "native-frame pipeline registry lock failed",
                        ),
                    );
                    return PlayerFfiCallStatus::Error;
                }
            };
            let Some(session) = sessions.get(handle).cloned() else {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        "invalid native-frame pipeline handle",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            };
            session
        };
        let mut session = match session.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Err(error) = session.flush() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::BackendFailure, &error),
            );
            return PlayerFfiCallStatus::Error;
        }
        let json =
            match native_frame_pipeline_status_json(handle, &*session, Some("flushed".to_owned())) {
                Ok(value) => value,
                Err(error) => {
                    write_error(
                        out_error,
                        owned_api_error(PlayerFfiErrorCode::BackendFailure, &error.to_string()),
                    );
                    return PlayerFfiCallStatus::Error;
                }
            };
        unsafe {
            ptr::write(out_json, into_c_string_ptr(json));
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// The handle must have been returned by `player_ffi_ios_native_frame_pipeline_open`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_ios_native_frame_pipeline_seek(
    handle: u64,
    position_millis: u64,
    out_json: *mut *mut c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if out_json.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_json was null"),
            );
            return PlayerFfiCallStatus::Error;
        }
        unsafe {
            ptr::write(out_json, ptr::null_mut());
        }
        // Clone the Arc under the registry lock, then drop the registry lock
        // before `seek_to()` (which flushes and re-primes decoder/packet plugin FFI).
        let session = {
            let sessions = match native_frame_pipeline_sessions().lock() {
                Ok(sessions) => sessions,
                Err(_) => {
                    write_error(
                        out_error,
                        owned_api_error(
                            PlayerFfiErrorCode::InvalidState,
                            "native-frame pipeline registry lock failed",
                        ),
                    );
                    return PlayerFfiCallStatus::Error;
                }
            };
            let Some(session) = sessions.get(handle).cloned() else {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidArgument,
                        "invalid native-frame pipeline handle",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            };
            session
        };
        let mut session = match session.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Err(error) = session.seek_to(position_millis) {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::SeekFailure, &error),
            );
            return PlayerFfiCallStatus::Error;
        }
        let json = match native_frame_pipeline_status_json(
            handle,
            &*session,
            Some(format!("seeked to {position_millis} ms")),
        ) {
            Ok(value) => value,
            Err(error) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::BackendFailure, &error.to_string()),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        unsafe {
            ptr::write(out_json, into_c_string_ptr(json));
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// The handle must have been returned by `player_ffi_ios_native_frame_pipeline_open`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_ios_native_frame_pipeline_close(handle: u64) {
    ffi_void(|| {
        // Remove the entry under the registry lock, but drop the returned
        // `IosNativeFramePipelineSession` (whose `Drop` runs plugin FFI close on
        // the decoder/packet/processor chain) *after* releasing the registry
        // lock. Dropping it under the lock would serialize every native-frame
        // pipeline session process-wide and block open/close for the teardown.
        let session = {
            if let Ok(mut sessions) = native_frame_pipeline_sessions().lock() {
                sessions.remove(handle)
            } else {
                None
            }
        };
        drop(session);
    });
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_dash_bridge_execute_json(
    request_json: *const c_char,
    out_json: *mut *mut c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if request_json.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "request_json was null"),
            );
            return PlayerFfiCallStatus::Error;
        }
        if out_json.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_json was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let request_json = match unsafe { CStr::from_ptr(request_json) }.to_str() {
            Ok(value) => value,
            Err(_) => {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::InvalidUtf8,
                        "request_json was not valid UTF-8",
                    ),
                );
                return PlayerFfiCallStatus::Error;
            }
        };

        let response_json = match player_dash_hls_bridge::ops::execute_json(request_json) {
            Ok(value) => value,
            Err(error) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::InvalidArgument, &error.to_string()),
                );
                return PlayerFfiCallStatus::Error;
            }
        };

        unsafe {
            ptr::write(out_json, into_c_string_ptr(response_json));
        }
        PlayerFfiCallStatus::Ok
    })
}

fn source_normalizer_mode_from_u32(value: u32) -> SourceNormalizerMode {
    match value {
        1 => SourceNormalizerMode::DiagnosticsOnly,
        2 => SourceNormalizerMode::PreflightOnly,
        3 => SourceNormalizerMode::PreferNormalized,
        4 => SourceNormalizerMode::RequireNormalized,
        _ => SourceNormalizerMode::Disabled,
    }
}

fn frame_processor_mode_from_u32(value: u32) -> FrameProcessorMode {
    match value {
        1 => FrameProcessorMode::DiagnosticsOnly,
        _ => FrameProcessorMode::Disabled,
    }
}

fn native_frame_pipeline_mode_from_u32(value: u32) -> NativeFramePipelineMode {
    match value {
        1 => NativeFramePipelineMode::DiagnosticsOnly,
        2 => NativeFramePipelineMode::PreferNativeFrame,
        3 => NativeFramePipelineMode::RequireNativeFrame,
        _ => NativeFramePipelineMode::Disabled,
    }
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_dash_bridge_parse_sidx(
    data: *const u8,
    data_len: usize,
    out_json: *mut *mut c_char,
    out_error: *mut PlayerFfiError,
) -> PlayerFfiCallStatus {
    ffi_call(out_error, || {
        if data.is_null() && data_len > 0 {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "data was null"),
            );
            return PlayerFfiCallStatus::Error;
        }
        if out_json.is_null() {
            write_error(
                out_error,
                owned_api_error(PlayerFfiErrorCode::NullPointer, "out_json was null"),
            );
            return PlayerFfiCallStatus::Error;
        }

        let data = if data_len == 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(data, data_len) }
        };
        let sidx = match player_dash_hls_bridge::mp4::parse_sidx(data) {
            Ok(value) => value,
            Err(error) => {
                write_error(
                    out_error,
                    owned_api_error(PlayerFfiErrorCode::InvalidArgument, &error.to_string()),
                );
                return PlayerFfiCallStatus::Error;
            }
        };
        let response_json = match serde_json::to_string(&sidx) {
            Ok(value) => value,
            Err(error) => {
                write_error(
                    out_error,
                    owned_api_error(
                        PlayerFfiErrorCode::BackendFailure,
                        &format!("failed to encode SIDX response: {error}"),
                    ),
                );
                return PlayerFfiCallStatus::Error;
            }
        };

        unsafe {
            ptr::write(out_json, into_c_string_ptr(response_json));
        }
        PlayerFfiCallStatus::Ok
    })
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_dash_bridge_string_free(value: *mut c_char) {
    ffi_void(|| {
        let mut value = value;
        free_c_string(&mut value);
    });
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_error_free(error: *mut PlayerFfiError) {
    ffi_void(|| {
        let Some(error) = (unsafe { error.as_mut() }) else {
            return;
        };

        free_c_string(&mut error.message);
        *error = PlayerFfiError::default();
    });
}

/// # Safety
///
/// Raw pointers and opaque handles passed to this FFI entry point must either be null when
/// the parameter is documented as optional or point to valid objects allocated by the
/// matching Vesper FFI API for the duration of the call. Callers must serialize shared
/// handle access according to the host binding contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_track_preferences_free(
    track_preferences: *mut PlayerFfiTrackPreferences,
) {
    ffi_void(|| {
        let Some(track_preferences) = (unsafe { track_preferences.as_mut() }) else {
            return;
        };

        free_c_string(&mut track_preferences.preferred_audio_language);
        free_c_string(&mut track_preferences.preferred_subtitle_language);
        free_c_string(&mut track_preferences.audio_selection.track_id);
        free_c_string(&mut track_preferences.subtitle_selection.track_id);
        free_c_string(&mut track_preferences.abr_policy.track_id);
        *track_preferences = PlayerFfiTrackPreferences::default();
    });
}
