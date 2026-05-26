use std::any::Any;
use std::borrow::Borrow;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, MutexGuard};

use jni::EnvUnowned;
use jni::errors::{Result as JniResult, ThrowRuntimeExAndDefault};
use jni::sys::jlong;

#[derive(Debug)]
pub(crate) struct HandleRegistry<T> {
    pub(crate) slots: Vec<HandleSlot<T>>,
    pub(crate) free_slots: Vec<u32>,
    next_generation_seed: u32,
}

#[derive(Debug)]
pub(crate) struct HandleSlot<T> {
    pub(crate) generation: u32,
    pub(crate) value: Option<T>,
}

impl<T> Default for HandleRegistry<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free_slots: Vec::new(),
            next_generation_seed: 0,
        }
    }
}

impl<T> HandleRegistry<T> {
    fn allocate_generation(&mut self) -> u32 {
        let generation = next_generation(self.next_generation_seed);
        self.next_generation_seed = generation;
        generation
    }

    pub(crate) fn insert(&mut self, value: T) -> i64 {
        let generation = self.allocate_generation();
        if let Some(slot_index) = self.free_slots.pop() {
            let slot = &mut self.slots[slot_index as usize];
            slot.generation = generation;
            slot.value = Some(value);
            return encode_handle(slot_index, generation);
        }

        debug_assert!(
            self.slots.len() < u32::MAX as usize,
            "HandleRegistry exhausted u32 slot space"
        );
        if self.slots.len() >= u32::MAX as usize {
            return 0;
        }

        let slot_index = self.slots.len() as u32;
        self.slots.push(HandleSlot {
            generation,
            value: Some(value),
        });
        encode_handle(slot_index, generation)
    }

    pub(crate) fn get(&self, handle: impl Borrow<i64>) -> Option<&T> {
        let (slot_index, generation) = decode_handle(*handle.borrow())?;
        let slot = self.slots.get(slot_index as usize)?;
        (slot.generation == generation)
            .then_some(slot.value.as_ref())
            .flatten()
    }

    pub(crate) fn get_mut(&mut self, handle: impl Borrow<i64>) -> Option<&mut T> {
        let (slot_index, generation) = decode_handle(*handle.borrow())?;
        let slot = self.slots.get_mut(slot_index as usize)?;
        (slot.generation == generation)
            .then_some(slot.value.as_mut())
            .flatten()
    }

    pub(crate) fn remove(&mut self, handle: impl Borrow<i64>) -> Option<T> {
        let (slot_index, generation) = decode_handle(*handle.borrow())?;
        let slot = self.slots.get_mut(slot_index as usize)?;
        if slot.generation != generation {
            return None;
        }
        let value = slot.value.take()?;
        self.free_slots.push(slot_index);
        self.compact_tail();
        Some(value)
    }

    fn compact_tail(&mut self) {
        let Some(last_used_index) = self.slots.iter().rposition(|slot| slot.value.is_some()) else {
            self.slots.clear();
            self.free_slots.clear();
            return;
        };

        let new_len = last_used_index + 1;
        if new_len == self.slots.len() {
            return;
        }

        self.slots.truncate(new_len);
        self.free_slots
            .retain(|slot_index| (*slot_index as usize) < new_len);
    }
}

fn encode_handle(slot_index: u32, generation: u32) -> i64 {
    let slot_id = u64::from(slot_index) + 1;
    let raw = (slot_id << 32) | u64::from(generation.max(1));
    raw as i64
}

fn decode_handle(handle: i64) -> Option<(u32, u32)> {
    if handle == 0 {
        return None;
    }
    let raw = handle as u64;
    let slot_id = (raw >> 32) as u32;
    let generation = raw as u32;
    if slot_id == 0 || generation == 0 {
        return None;
    }
    Some((slot_id - 1, generation))
}

pub(crate) fn next_generation(generation: u32) -> u32 {
    generation.wrapping_add(1).max(1)
}

pub(crate) fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return format!("Rust panic crossed JNI boundary: {message}");
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return format!("Rust panic crossed JNI boundary: {message}");
    }
    "Rust panic crossed JNI boundary".to_owned()
}

fn throw_panic_exception(unowned_env: &mut EnvUnowned<'_>, message: &str) {
    unowned_env
        .with_env(|env| -> JniResult<()> {
            env.throw_new(
                crate::jni_name("java/lang/RuntimeException"),
                crate::jni_name(message),
            )?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

pub(crate) fn run_jni_entry<T: Default>(
    unowned_env: &mut EnvUnowned<'_>,
    f: impl FnOnce(&mut EnvUnowned<'_>) -> T,
) -> T {
    match catch_unwind(AssertUnwindSafe(|| f(unowned_env))) {
        Ok(value) => value,
        Err(payload) => {
            let message = panic_message(payload.as_ref());
            throw_panic_exception(unowned_env, &message);
            T::default()
        }
    }
}

pub(crate) fn u64_to_jlong_saturating(value: u64) -> jlong {
    value.min(i64::MAX as u64) as jlong
}

pub(crate) fn u128_to_jlong_saturating(value: u128) -> jlong {
    value.min(i64::MAX as u128) as jlong
}
