use std::borrow::Borrow;
use std::sync::{Mutex, OnceLock};

use player_platform_ios::{
    IosDownloadBridgeSession, IosPlaylistBridgeSession, IosPreloadBridgeSession,
};
use player_platform_mobile::MobileSourceNormalizerResourceOpen;
use player_plugin_loader::BenchmarkSinkPluginSession;

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
static DOWNLOAD_SESSIONS: OnceLock<Mutex<HandleRegistry<IosDownloadBridgeSession>>> =
    OnceLock::new();
static PLAYLIST_SESSIONS: OnceLock<Mutex<HandleRegistry<IosPlaylistBridgeSession>>> =
    OnceLock::new();
static BENCHMARK_SESSIONS: OnceLock<Mutex<HandleRegistry<BenchmarkSinkPluginSession>>> =
    OnceLock::new();
static SOURCE_NORMALIZER_RESOURCE_SESSIONS: OnceLock<
    Mutex<HandleRegistry<MobileSourceNormalizerResourceOpen>>,
> = OnceLock::new();

pub(crate) fn preload_sessions() -> &'static Mutex<HandleRegistry<IosPreloadBridgeSession>> {
    PRELOAD_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

pub(crate) fn download_sessions() -> &'static Mutex<HandleRegistry<IosDownloadBridgeSession>> {
    DOWNLOAD_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

pub(crate) fn playlist_sessions() -> &'static Mutex<HandleRegistry<IosPlaylistBridgeSession>> {
    PLAYLIST_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

pub(crate) fn benchmark_sessions() -> &'static Mutex<HandleRegistry<BenchmarkSinkPluginSession>> {
    BENCHMARK_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}

pub(crate) fn source_normalizer_resource_sessions()
-> &'static Mutex<HandleRegistry<MobileSourceNormalizerResourceOpen>> {
    SOURCE_NORMALIZER_RESOURCE_SESSIONS.get_or_init(|| Mutex::new(HandleRegistry::default()))
}
