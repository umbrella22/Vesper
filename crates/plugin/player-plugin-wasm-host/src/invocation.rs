use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::{WasmPluginHostError, WasmQueueError, WasmWaitOperation};

/// Receipt for accepted work. Dropping it or expiring a wait does not cancel
/// execution. The queue publishes the eventual result to a bounded report sink;
/// a full sink drops the newest report and increments its loss counter.
#[derive(Debug)]
#[must_use = "accepted work runs even if its receipt is dropped"]
pub struct WasmInvocationTicket<T> {
    invocation_id: u64,
    result: Receiver<T>,
    operation: WasmWaitOperation,
    wait_expirations: Arc<AtomicU64>,
}

impl<T> WasmInvocationTicket<T> {
    pub(crate) fn new(
        invocation_id: u64,
        result: Receiver<T>,
        operation: WasmWaitOperation,
        wait_expirations: Arc<AtomicU64>,
    ) -> Self {
        Self {
            invocation_id,
            result,
            operation,
            wait_expirations,
        }
    }

    /// Monotonic accepted-work ID, unique within this queue only.
    pub fn invocation_id(&self) -> u64 {
        self.invocation_id
    }

    /// Consumes only the caller's receipt, never the accepted invocation.
    pub fn wait(self, timeout: Duration) -> Result<T, WasmPluginHostError> {
        match self.result.recv_timeout(timeout) {
            Ok(report) => Ok(report),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ =
                    self.wait_expirations
                        .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                            Some(n.saturating_add(1))
                        });
                Err(WasmPluginHostError::WaitExpired {
                    operation: self.operation,
                    invocation_id: Some(self.invocation_id),
                })
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(WasmPluginHostError::Queue(WasmQueueError::WorkerStopped))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WasmQueueLifecycle {
    #[default]
    Open,
    Closing,
    Closed,
}

/// Cumulative queue accounting, independent of the bounded diagnostic drain.
/// `pending_invocations` remain isolated when close expires. Wait expiry is not
/// an execution timeout; inspect the typed invocation result for that cause.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WasmQueueSettlement {
    pub lifecycle: WasmQueueLifecycle,
    pub accepted_invocations: u64,
    pub completed_invocations: u64,
    /// Failed event/batch results, including author failures. Guest flush calls
    /// do not contribute to invocation counters.
    pub failed_invocations: u64,
    /// Result-ticket waits that expired. Flush/close wait errors are not counted.
    /// Dropping a ticket without waiting is not a wait expiry.
    pub wait_expirations: u64,
    pub pending_invocations: u64,
    /// Explicit close has joined the worker. Closed alone does not prove a join.
    pub worker_joined: bool,
    pub quarantine_reason: Option<WasmPluginHostError>,
}

#[derive(Debug)]
enum WorkerJoinState {
    Running(JoinHandle<()>),
    Joining,
    Joined(Result<(), WasmPluginHostError>),
}

/// Shared mechanics only: no EventHook/Benchmark business or flushing policy.
#[derive(Debug)]
pub(crate) struct WasmWorker {
    state: Mutex<WorkerJoinState>,
    changed: Condvar,
}

impl WasmWorker {
    pub(crate) fn new(worker: JoinHandle<()>) -> Self {
        Self {
            state: Mutex::new(WorkerJoinState::Running(worker)),
            changed: Condvar::new(),
        }
    }

    pub(crate) fn is_joined(&self) -> bool {
        matches!(
            *self.state.lock().unwrap_or_else(|p| p.into_inner()),
            WorkerJoinState::Joined(_)
        )
    }

    pub(crate) fn join(
        &self,
        started: Instant,
        timeout: Duration,
    ) -> Result<(), WasmPluginHostError> {
        loop {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            match &*state {
                WorkerJoinState::Joined(result) => return result.clone(),
                WorkerJoinState::Running(worker) if worker.is_finished() => {
                    let previous = std::mem::replace(&mut *state, WorkerJoinState::Joining);
                    let WorkerJoinState::Running(worker) = previous else {
                        *state = previous;
                        return Err(WasmPluginHostError::Queue(WasmQueueError::InvalidState));
                    };
                    drop(state);
                    let result = worker
                        .join()
                        .map_err(|_| WasmPluginHostError::WorkerPanic("worker join".to_owned()));
                    let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
                    *state = WorkerJoinState::Joined(result.clone());
                    self.changed.notify_all();
                    return result;
                }
                WorkerJoinState::Running(_) | WorkerJoinState::Joining => {
                    let Some(remaining) = timeout.checked_sub(started.elapsed()) else {
                        return Err(WasmPluginHostError::WaitExpired {
                            operation: WasmWaitOperation::WorkerJoin,
                            invocation_id: None,
                        });
                    };
                    // A finishing worker does not own this condition variable.
                    // Poll only the join readiness, never guest execution.
                    let wait = remaining.min(Duration::from_millis(1));
                    let _guard = self
                        .changed
                        .wait_timeout(state, wait)
                        .unwrap_or_else(|p| p.into_inner());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_expiry_keeps_correlation_and_does_not_cancel_work() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let waits = Arc::new(AtomicU64::new(0));
        let ticket = WasmInvocationTicket::new(
            42,
            receiver,
            WasmWaitOperation::EventResult,
            Arc::clone(&waits),
        );
        assert_eq!(ticket.invocation_id(), 42);
        assert_eq!(
            ticket.wait(Duration::ZERO),
            Err(WasmPluginHostError::WaitExpired {
                operation: WasmWaitOperation::EventResult,
                invocation_id: Some(42)
            })
        );
        assert_eq!(waits.load(Ordering::Relaxed), 1);
        // The worker can still finish; the discarded waiter is not cancellation.
        assert!(matches!(
            sender.try_send(7),
            Err(mpsc::TrySendError::Disconnected(7))
        ));
    }

    #[test]
    fn completed_receipt_and_worker_stop_are_not_wait_expiry() {
        let (sender, receiver) = mpsc::sync_channel(1);
        sender.try_send(9).expect("completed receipt");
        let waits = Arc::new(AtomicU64::new(0));
        let ticket = WasmInvocationTicket::new(
            1,
            receiver,
            WasmWaitOperation::BatchResult,
            Arc::clone(&waits),
        );
        assert_eq!(ticket.wait(Duration::ZERO), Ok(9));
        let (sender, receiver) = mpsc::sync_channel::<u64>(1);
        drop(sender);
        let ticket = WasmInvocationTicket::new(
            2,
            receiver,
            WasmWaitOperation::BatchResult,
            Arc::clone(&waits),
        );
        assert_eq!(
            ticket.wait(Duration::ZERO),
            Err(WasmPluginHostError::Queue(WasmQueueError::WorkerStopped))
        );
        assert_eq!(waits.load(Ordering::Relaxed), 0);
    }
}
