use std::borrow::Borrow;
use std::sync::{Arc, LockResult, Mutex, MutexGuard, OnceLock};

use player_platform_ios::{
    IosDownloadBridgeSession, IosPlaylistBridgeSession, IosPreloadBridgeSession,
    IosSequenceBridgeSession,
};
use player_platform_mobile::MobileSourceNormalizerResourceOpen;
use player_plugin_loader::BenchmarkSinkPluginSession;
use player_runtime::PipelineEventDispatcher;

use crate::native_frame_pipeline::IosNativeFramePipelineSession;

/// Source-normalizer resource sessions wrap blocking plugin `poll()` calls that
/// perform filesystem I/O (directory walks, disk-usage scans). They are stored
/// behind an `Arc<Mutex<...>>` so accessors can clone the `Arc` under the global
/// registry lock, drop the registry lock, and only then take the per-session
/// lock while running caller closures. This keeps blocking I/O off the global
/// registry mutex.
pub(crate) type IosSourceNormalizerResourceSession = Arc<Mutex<MobileSourceNormalizerResourceOpen>>;

/// Benchmark sink sessions invoke plugin FFI calls (`on_event_batch`/`flush`)
/// that may block on a dlopen-loaded plugin. They are stored behind an
/// `Arc<Mutex<...>>` so accessors clone the `Arc` under the global registry
/// lock, drop the registry lock, and only then take the per-session lock while
/// running caller closures. This keeps plugin FFI off the global registry
/// mutex, mirroring the JNI `AndroidBenchmarkSinkSession` pattern.
pub(crate) type IosBenchmarkSinkSession = Arc<Mutex<BenchmarkSinkPluginSession>>;

/// Playback EventHook sessions are kept behind an `Arc<Mutex<...>>` so the
/// generation-safe handle registry can clone the session under its lock and
/// execute plugin dispatch/flush work after releasing the global lock.
pub(crate) type IosPlaybackEventHookSession = Arc<Mutex<PipelineEventDispatcher>>;

/// Native-frame pipeline sessions perform blocking VideoToolbox decode,
/// plugin FFI (decoder/packet/processor flush + close), and EOS-drain sleeps
/// during `advance`/`flush`/`seek`/`release_pending_frame`. They are stored
/// behind an `Arc<Mutex<...>>` so accessors clone the `Arc` under the global
/// registry lock, drop the registry lock, and only then take the per-session
/// lock while running caller closures. This keeps blocking decode/plugin work
/// off the global registry mutex, mirroring the JNI
/// `AndroidNativeFramePipelineSession` pattern.
pub(crate) type IosNativeFramePipelineSessionHandle = Arc<Mutex<IosNativeFramePipelineSession>>;

/// Download bridge sessions hold a `DownloadManager` whose post-processor chain
/// (`DynamicPostDownloadProcessor`) performs blocking plugin FFI (`process_json`
/// during complete/export, `destroy` during Drop). They are stored behind an
/// `Arc<Mutex<...>>` so accessors clone the `Arc` under the global registry lock,
/// drop the registry lock, and only then take the per-session lock while running
/// caller closures. This keeps blocking plugin post-processing off the global
/// registry mutex, mirroring the JNI `with_download_session_mut` pattern.
pub(crate) type IosDownloadBridgeSessionHandle = Arc<Mutex<IosDownloadBridgeSession>>;
pub(crate) type IosSequenceBridgeSessionHandle = Arc<Mutex<IosSequenceBridgeSession>>;

#[derive(Debug)]
pub(crate) struct HandleRegistry<T> {
    slots: Vec<HandleSlot<T>>,
    free_slots: Vec<u32>,
}

#[derive(Debug)]
struct HandleSlot<T> {
    generation: u32,
    value: Option<T>,
}

impl<T> Default for HandleRegistry<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free_slots: Vec::new(),
        }
    }
}

impl<T> HandleRegistry<T> {
    pub(crate) fn len(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.value.is_some())
            .count()
    }

    pub(crate) fn insert(&mut self, value: T) -> u64 {
        if let Some(slot_index) = self.free_slots.pop() {
            let slot = &mut self.slots[slot_index as usize];
            slot.generation = next_generation(slot.generation);
            slot.value = Some(value);
            return encode_handle(slot_index, slot.generation);
        }

        let slot_index = self.slots.len() as u32;
        self.slots.push(HandleSlot {
            generation: 1,
            value: Some(value),
        });
        encode_handle(slot_index, 1)
    }

    pub(crate) fn get(&self, handle: impl Borrow<u64>) -> Option<&T> {
        let (slot_index, generation) = decode_handle(*handle.borrow())?;
        let slot = self.slots.get(slot_index as usize)?;
        (slot.generation == generation)
            .then_some(slot.value.as_ref())
            .flatten()
    }

    pub(crate) fn get_mut(&mut self, handle: impl Borrow<u64>) -> Option<&mut T> {
        let (slot_index, generation) = decode_handle(*handle.borrow())?;
        let slot = self.slots.get_mut(slot_index as usize)?;
        (slot.generation == generation)
            .then_some(slot.value.as_mut())
            .flatten()
    }

    pub(crate) fn remove(&mut self, handle: impl Borrow<u64>) -> Option<T> {
        let (slot_index, generation) = decode_handle(*handle.borrow())?;
        let slot = self.slots.get_mut(slot_index as usize)?;
        if slot.generation != generation {
            return None;
        }
        let value = slot.value.take()?;
        self.free_slots.push(slot_index);
        Some(value)
    }
}

pub(crate) fn encode_handle(slot_index: u32, generation: u32) -> u64 {
    let slot_id = u64::from(slot_index) + 1;
    (slot_id << 32) | u64::from(generation.max(1))
}

pub(crate) fn decode_handle(handle: u64) -> Option<(u32, u32)> {
    if handle == 0 {
        return None;
    }
    let slot_id = (handle >> 32) as u32;
    let generation = handle as u32;
    if slot_id == 0 || generation == 0 {
        return None;
    }
    Some((slot_id - 1, generation))
}

pub(crate) fn next_generation(generation: u32) -> u32 {
    generation.wrapping_add(1).max(1)
}

static PRELOAD_SESSIONS: OnceLock<Mutex<HandleRegistry<IosPreloadBridgeSession>>> = OnceLock::new();
static DOWNLOAD_SESSIONS: OnceLock<Mutex<HandleRegistry<IosDownloadBridgeSessionHandle>>> =
    OnceLock::new();
static PLAYLIST_SESSIONS: OnceLock<Mutex<HandleRegistry<IosPlaylistBridgeSession>>> =
    OnceLock::new();
static SEQUENCE_SESSIONS: OnceLock<Mutex<HandleRegistry<IosSequenceBridgeSessionHandle>>> =
    OnceLock::new();
static BENCHMARK_SESSIONS: OnceLock<Mutex<HandleRegistry<IosBenchmarkSinkSession>>> =
    OnceLock::new();
static PLAYBACK_EVENT_HOOK_SESSIONS: OnceLock<Mutex<HandleRegistry<IosPlaybackEventHookSession>>> =
    OnceLock::new();
static SOURCE_NORMALIZER_RESOURCE_SESSIONS: OnceLock<
    Mutex<HandleRegistry<IosSourceNormalizerResourceSession>>,
> = OnceLock::new();
static NATIVE_FRAME_PIPELINE_SESSIONS: OnceLock<
    Mutex<HandleRegistry<IosNativeFramePipelineSessionHandle>>,
> = OnceLock::new();

pub(crate) fn lock_registry<T>(registry: &Mutex<T>) -> LockResult<MutexGuard<'_, T>> {
    match registry.lock() {
        Ok(guard) => Ok(guard),
        Err(poisoned) => Ok(poisoned.into_inner()),
    }
}

pub(crate) fn preload_sessions() -> &'static Mutex<HandleRegistry<IosPreloadBridgeSession>> {
    PRELOAD_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

pub(crate) fn download_sessions() -> &'static Mutex<HandleRegistry<IosDownloadBridgeSessionHandle>>
{
    DOWNLOAD_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

pub(crate) fn playlist_sessions() -> &'static Mutex<HandleRegistry<IosPlaylistBridgeSession>> {
    PLAYLIST_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

pub(crate) fn sequence_sessions() -> &'static Mutex<HandleRegistry<IosSequenceBridgeSessionHandle>>
{
    SEQUENCE_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

pub(crate) fn benchmark_sessions() -> &'static Mutex<HandleRegistry<IosBenchmarkSinkSession>> {
    BENCHMARK_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

pub(crate) fn playback_event_hook_sessions()
-> &'static Mutex<HandleRegistry<IosPlaybackEventHookSession>> {
    PLAYBACK_EVENT_HOOK_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

pub(crate) fn source_normalizer_resource_sessions()
-> &'static Mutex<HandleRegistry<IosSourceNormalizerResourceSession>> {
    SOURCE_NORMALIZER_RESOURCE_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

pub(crate) fn native_frame_pipeline_sessions()
-> &'static Mutex<HandleRegistry<IosNativeFramePipelineSessionHandle>> {
    NATIVE_FRAME_PIPELINE_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}
