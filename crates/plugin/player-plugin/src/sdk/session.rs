use std::mem;
use std::sync::{Arc, Mutex};

use player_plugin_abi::VESPER_MAX_SESSIONS_PER_INTERFACE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionRegistryError {
    Stale,
    Busy,
    Exhausted,
}

enum SessionState<S> {
    Open(S),
    Busy,
    Closed,
}

struct SessionSlot<S> {
    generation: u32,
    state: Arc<Mutex<SessionState<S>>>,
}

pub(super) struct SessionRegistry<S> {
    slots: Vec<SessionSlot<S>>,
}

impl<S> Default for SessionRegistry<S> {
    fn default() -> Self {
        Self { slots: Vec::new() }
    }
}

impl<S> SessionRegistry<S> {
    pub(super) fn insert(&mut self, value: S) -> Result<u64, (SessionRegistryError, S)> {
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if slot.generation == u32::MAX {
                continue;
            }
            let state = slot.state.lock().unwrap_or_else(|error| error.into_inner());
            if !matches!(*state, SessionState::Closed) {
                continue;
            }
            drop(state);
            let generation = slot.generation + 1;
            let token = match encode_token(index, generation) {
                Ok(token) => token,
                Err(error) => return Err((error, value)),
            };
            slot.generation = generation;
            slot.state = Arc::new(Mutex::new(SessionState::Open(value)));
            return Ok(token);
        }

        if self.slots.len() >= VESPER_MAX_SESSIONS_PER_INTERFACE {
            return Err((SessionRegistryError::Exhausted, value));
        }
        let generation = 1;
        let index = self.slots.len();
        let token = match encode_token(index, generation) {
            Ok(token) => token,
            Err(error) => return Err((error, value)),
        };
        self.slots.push(SessionSlot {
            generation,
            state: Arc::new(Mutex::new(SessionState::Open(value))),
        });
        Ok(token)
    }

    pub(super) fn acquire(&self, token: u64) -> Result<SessionGuard<S>, SessionRegistryError> {
        let state = self.resolve(token)?;
        let mut locked = state.lock().unwrap_or_else(|error| error.into_inner());
        match mem::replace(&mut *locked, SessionState::Busy) {
            SessionState::Open(value) => {
                drop(locked);
                Ok(SessionGuard {
                    state,
                    value: Some(value),
                })
            }
            SessionState::Busy => {
                *locked = SessionState::Busy;
                Err(SessionRegistryError::Busy)
            }
            SessionState::Closed => {
                *locked = SessionState::Closed;
                Err(SessionRegistryError::Stale)
            }
        }
    }

    pub(super) fn begin_close(
        &self,
        token: u64,
    ) -> Result<Option<SessionCloseGuard<S>>, SessionRegistryError> {
        let state = self.resolve(token)?;
        let mut locked = state.lock().unwrap_or_else(|error| error.into_inner());
        match mem::replace(&mut *locked, SessionState::Busy) {
            SessionState::Open(value) => {
                drop(locked);
                Ok(Some(SessionCloseGuard {
                    state,
                    value: Some(value),
                    committed: false,
                }))
            }
            SessionState::Closed => {
                *locked = SessionState::Closed;
                Ok(None)
            }
            SessionState::Busy => {
                *locked = SessionState::Busy;
                Err(SessionRegistryError::Busy)
            }
        }
    }

    fn resolve(&self, token: u64) -> Result<Arc<Mutex<SessionState<S>>>, SessionRegistryError> {
        let (index, generation) = decode_token(token)?;
        let slot = self.slots.get(index).ok_or(SessionRegistryError::Stale)?;
        if slot.generation != generation {
            return Err(SessionRegistryError::Stale);
        }
        Ok(slot.state.clone())
    }
}

pub(super) struct SessionGuard<S> {
    state: Arc<Mutex<SessionState<S>>>,
    value: Option<S>,
}

impl<S> SessionGuard<S> {
    pub(super) fn value_mut(&mut self) -> Option<&mut S> {
        self.value.as_mut()
    }
}

impl<S> Drop for SessionGuard<S> {
    fn drop(&mut self) {
        let Some(value) = self.value.take() else {
            return;
        };
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if matches!(*state, SessionState::Busy) {
            *state = SessionState::Open(value);
        }
    }
}

pub(super) struct SessionCloseGuard<S> {
    state: Arc<Mutex<SessionState<S>>>,
    value: Option<S>,
    committed: bool,
}

impl<S> SessionCloseGuard<S> {
    pub(super) fn value_mut(&mut self) -> Option<&mut S> {
        self.value.as_mut()
    }

    pub(super) fn commit(mut self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if matches!(*state, SessionState::Busy) {
            *state = SessionState::Closed;
        }
        self.value.take();
        self.committed = true;
    }
}

impl<S> Drop for SessionCloseGuard<S> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let Some(value) = self.value.take() else {
            return;
        };
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if matches!(*state, SessionState::Busy) {
            *state = SessionState::Open(value);
        }
    }
}

fn encode_token(index: usize, generation: u32) -> Result<u64, SessionRegistryError> {
    let slot = u32::try_from(index)
        .ok()
        .and_then(|index| index.checked_add(1))
        .ok_or(SessionRegistryError::Exhausted)?;
    Ok((u64::from(generation) << 32) | u64::from(slot))
}

fn decode_token(token: u64) -> Result<(usize, u32), SessionRegistryError> {
    let generation = (token >> 32) as u32;
    let slot = token as u32;
    if generation == 0 || slot == 0 {
        return Err(SessionRegistryError::Stale);
    }
    Ok(((slot - 1) as usize, generation))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_nonzero_and_reuse_increments_generation() {
        let mut registry = SessionRegistry::default();
        let first = registry.insert(10).expect("insert first");
        assert_ne!(first, 0);
        registry
            .begin_close(first)
            .expect("begin close")
            .expect("open session")
            .commit();
        assert!(
            registry
                .begin_close(first)
                .expect("closed lookup")
                .is_none()
        );

        let second = registry.insert(20).expect("reuse slot");
        assert_ne!(second, first);
        assert!(matches!(
            registry.acquire(first),
            Err(SessionRegistryError::Stale)
        ));
        assert_eq!(
            *registry
                .acquire(second)
                .expect("new token")
                .value_mut()
                .expect("open value"),
            20
        );
    }

    #[test]
    fn guard_returns_the_session_after_unwind() {
        let mut registry = SessionRegistry::default();
        let token = registry.insert(String::from("open")).expect("insert");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut session = registry.acquire(token).expect("acquire");
            session
                .value_mut()
                .expect("open value")
                .push_str("-mutated");
            panic!("fixture panic");
        }));
        assert!(result.is_err());
        assert_eq!(
            registry
                .acquire(token)
                .expect("reacquire")
                .value_mut()
                .expect("open value"),
            "open-mutated"
        );
    }

    #[test]
    fn concurrent_use_is_reported_as_busy() {
        let mut registry = SessionRegistry::default();
        let token = registry.insert(1).expect("insert");
        let _guard = registry.acquire(token).expect("first acquire");
        assert!(matches!(
            registry.acquire(token),
            Err(SessionRegistryError::Busy)
        ));
        assert_eq!(
            registry.begin_close(token).map(|guard| guard.is_some()),
            Err(SessionRegistryError::Busy)
        );
    }

    #[test]
    fn uncommitted_close_restores_the_session_for_retry() {
        let mut registry = SessionRegistry::default();
        let token = registry.insert(String::from("open")).expect("insert");
        {
            let mut closing = registry
                .begin_close(token)
                .expect("begin close")
                .expect("open session");
            closing
                .value_mut()
                .expect("closing value")
                .push_str("-first-attempt");
        }
        let mut retry = registry
            .begin_close(token)
            .expect("retry close")
            .expect("restored session");
        assert_eq!(
            retry.value_mut().expect("retry value"),
            "open-first-attempt"
        );
        retry.commit();
        assert!(
            registry
                .begin_close(token)
                .expect("closed lookup")
                .is_none()
        );
    }
}
