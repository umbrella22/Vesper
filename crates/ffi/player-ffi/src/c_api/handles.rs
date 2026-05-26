use super::*;

#[derive(Debug)]
struct HandleSlot<T> {
    generation: u32,
    value: T,
}

#[derive(Debug)]
struct HandleRegistry<T> {
    slots: Vec<Option<HandleSlot<T>>>,
    next_generation_seed: u32,
}

impl<T> HandleRegistry<T> {
    fn insert(&mut self, value: T) -> Result<u64, HandleRegistryError> {
        let generation = self.allocate_generation();
        if let Some((slot_index, slot)) = self
            .slots
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.is_none())
        {
            *slot = Some(HandleSlot { generation, value });
            let slot_index =
                u32::try_from(slot_index).map_err(|_| HandleRegistryError::TooManyHandles)?;
            return Ok(encode_registry_handle(slot_index, generation));
        }

        let slot_index =
            u32::try_from(self.slots.len()).map_err(|_| HandleRegistryError::TooManyHandles)?;
        self.slots.push(Some(HandleSlot { generation, value }));
        Ok(encode_registry_handle(slot_index, generation))
    }

    fn get(&self, handle: u64) -> Option<&T> {
        let (slot_index, generation) = decode_registry_handle(handle)?;
        let slot = self.slots.get(slot_index as usize)?.as_ref()?;
        (slot.generation == generation).then_some(&slot.value)
    }

    fn get_mut(&mut self, handle: u64) -> Option<&mut T> {
        let (slot_index, generation) = decode_registry_handle(handle)?;
        let slot = self.slots.get_mut(slot_index as usize)?.as_mut()?;
        (slot.generation == generation).then_some(&mut slot.value)
    }

    fn remove(&mut self, handle: u64) -> Option<T> {
        let (slot_index, generation) = decode_registry_handle(handle)?;
        let slot = self.slots.get_mut(slot_index as usize)?;
        let existing = slot.as_ref()?;
        if existing.generation != generation {
            return None;
        }

        let value = slot.take().map(|entry| entry.value);
        self.compact_tail();
        value
    }

    fn allocate_generation(&mut self) -> u32 {
        let generation = next_generation(self.next_generation_seed);
        self.next_generation_seed = generation;
        generation
    }

    fn compact_tail(&mut self) {
        while matches!(self.slots.last(), Some(None)) {
            self.slots.pop();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandleRegistryError {
    TooManyHandles,
}

impl<T> Default for HandleRegistry<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            next_generation_seed: 0,
        }
    }
}

static INITIALIZER_HANDLE_REGISTRY: OnceLock<Mutex<HandleRegistry<usize>>> = OnceLock::new();
static PLAYER_HANDLE_REGISTRY: OnceLock<Mutex<HandleRegistry<usize>>> = OnceLock::new();

fn lock_initializer_registry() -> std::sync::MutexGuard<'static, HandleRegistry<usize>> {
    lock_registry(INITIALIZER_HANDLE_REGISTRY.get_or_init(|| Mutex::new(HandleRegistry::default())))
}

fn lock_player_registry() -> std::sync::MutexGuard<'static, HandleRegistry<usize>> {
    lock_registry(PLAYER_HANDLE_REGISTRY.get_or_init(|| Mutex::new(HandleRegistry::default())))
}

fn lock_registry<T>(
    registry: &'static Mutex<HandleRegistry<T>>,
) -> std::sync::MutexGuard<'static, HandleRegistry<T>> {
    match registry.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub(crate) fn encode_registry_handle(slot_index: u32, generation: u32) -> u64 {
    ((u64::from(slot_index) + 1) << 32) | u64::from(generation.max(1))
}

pub(crate) fn decode_registry_handle(handle: u64) -> Option<(u32, u32)> {
    if handle == 0 {
        return None;
    }

    let slot_id = u32::try_from(handle >> 32).ok()?;
    let generation = handle as u32;
    if slot_id == 0 || generation == 0 {
        return None;
    }

    Some((slot_id - 1, generation))
}

pub(crate) fn next_generation(generation: u32) -> u32 {
    generation.wrapping_add(1).max(1)
}

pub(crate) fn into_initializer_handle(
    initializer: FfiPlayerInitializer,
) -> Option<PlayerFfiInitializerHandle> {
    let pointer = Box::into_raw(Box::new(initializer)) as usize;
    let raw = match lock_initializer_registry().insert(pointer) {
        Ok(raw) => raw,
        Err(HandleRegistryError::TooManyHandles) => {
            unsafe {
                drop(Box::from_raw(pointer as *mut FfiPlayerInitializer));
            }
            return None;
        }
    };
    Some(PlayerFfiInitializerHandle { raw })
}

pub(crate) fn into_player_handle(player: FfiPlayer) -> Option<PlayerFfiHandle> {
    let pointer = Box::into_raw(Box::new(player)) as usize;
    let raw = match lock_player_registry().insert(pointer) {
        Ok(raw) => raw,
        Err(HandleRegistryError::TooManyHandles) => {
            unsafe {
                drop(Box::from_raw(pointer as *mut FfiPlayer));
            }
            return None;
        }
    };
    Some(PlayerFfiHandle { raw })
}

pub(crate) fn with_initializer_ref<R>(
    handle: PlayerFfiInitializerHandle,
    f: impl FnOnce(&FfiPlayerInitializer) -> R,
) -> Option<R> {
    let registry = lock_initializer_registry();
    let pointer = registry.get(handle.raw).copied()?;
    unsafe { Some(f(&*(pointer as *const FfiPlayerInitializer))) }
}

pub(crate) fn take_initializer(handle: PlayerFfiInitializerHandle) -> Option<FfiPlayerInitializer> {
    let pointer = lock_initializer_registry().remove(handle.raw)?;
    unsafe { Some(*Box::from_raw(pointer as *mut FfiPlayerInitializer)) }
}

pub(crate) fn destroy_initializer_handle(handle: PlayerFfiInitializerHandle) -> bool {
    let Some(pointer) = lock_initializer_registry().remove(handle.raw) else {
        return false;
    };
    unsafe {
        drop(Box::from_raw(pointer as *mut FfiPlayerInitializer));
    }
    true
}

pub(crate) fn with_player_ref<R>(
    handle: PlayerFfiHandle,
    f: impl FnOnce(&FfiPlayer) -> R,
) -> Option<R> {
    let registry = lock_player_registry();
    let pointer = registry.get(handle.raw).copied()?;
    unsafe { Some(f(&*(pointer as *const FfiPlayer))) }
}

pub(crate) fn with_player_mut<R>(
    handle: PlayerFfiHandle,
    f: impl FnOnce(&mut FfiPlayer) -> R,
) -> Option<R> {
    let mut registry = lock_player_registry();
    let pointer = registry.get_mut(handle.raw).copied()?;
    unsafe { Some(f(&mut *(pointer as *mut FfiPlayer))) }
}

pub(crate) fn destroy_player_handle(handle: PlayerFfiHandle) -> bool {
    let Some(pointer) = lock_player_registry().remove(handle.raw) else {
        return false;
    };
    unsafe {
        drop(Box::from_raw(pointer as *mut FfiPlayer));
    }
    true
}

pub(crate) fn invalid_initializer_handle_error() -> PlayerFfiError {
    owned_api_error(
        PlayerFfiErrorCode::InvalidState,
        "initializer handle was invalid",
    )
}

pub(crate) fn invalid_player_handle_error() -> PlayerFfiError {
    owned_api_error(
        PlayerFfiErrorCode::InvalidState,
        "player handle was invalid",
    )
}

pub(crate) fn write_handle<T: Copy>(out_handle: *mut T, handle: T) {
    unsafe {
        ptr::write(out_handle, handle);
    }
}

pub(crate) fn write_default_if_non_null<T: Default>(out: *mut T) {
    if out.is_null() {
        return;
    }

    unsafe {
        ptr::write(out, T::default());
    }
}

pub(crate) fn error_mut(error: *mut PlayerFfiError) -> Option<&'static mut PlayerFfiError> {
    if error.is_null() {
        return None;
    }

    unsafe { Some(&mut *error) }
}

pub(crate) fn media_info_mut(
    media_info: *mut PlayerFfiMediaInfo,
) -> Option<&'static mut PlayerFfiMediaInfo> {
    if media_info.is_null() {
        return None;
    }

    unsafe { Some(&mut *media_info) }
}

pub(crate) fn track_preferences_mut(
    track_preferences: *mut PlayerFfiTrackPreferences,
) -> Option<&'static mut PlayerFfiTrackPreferences> {
    if track_preferences.is_null() {
        return None;
    }

    unsafe { Some(&mut *track_preferences) }
}

pub(crate) fn startup_mut(startup: *mut PlayerFfiStartup) -> Option<&'static mut PlayerFfiStartup> {
    if startup.is_null() {
        return None;
    }

    unsafe { Some(&mut *startup) }
}

pub(crate) fn snapshot_mut(
    snapshot: *mut PlayerFfiSnapshot,
) -> Option<&'static mut PlayerFfiSnapshot> {
    if snapshot.is_null() {
        return None;
    }

    unsafe { Some(&mut *snapshot) }
}

pub(crate) fn video_frame_mut(
    frame: *mut PlayerFfiVideoFrame,
) -> Option<&'static mut PlayerFfiVideoFrame> {
    if frame.is_null() {
        return None;
    }

    unsafe { Some(&mut *frame) }
}

pub(crate) fn event_list_mut(
    events: *mut PlayerFfiEventList,
) -> Option<&'static mut PlayerFfiEventList> {
    if events.is_null() {
        return None;
    }

    unsafe { Some(&mut *events) }
}
