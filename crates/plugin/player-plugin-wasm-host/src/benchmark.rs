use crate::invocation::WasmWorker;
use crate::{WasmInvocationTicket, WasmQueueLifecycle, WasmQueueSettlement, WasmWaitOperation};
use std::collections::{BTreeMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use player_plugin::{
    BenchmarkEvent, BenchmarkEventBatch, BenchmarkSinkReport, BenchmarkSinkStatus,
    BenchmarkThresholdViolation, MAX_BENCHMARK_THRESHOLD_VIOLATIONS, MAX_PLUGIN_DIAGNOSTICS,
    MAX_PLUGIN_ERROR_MESSAGE_BYTES, MAX_PLUGIN_MEASUREMENTS, PluginDiagnostic,
    PluginDiagnosticSeverity, PluginMeasurement,
};
use wasmtime::Store;
use wasmtime::component::{HasSelf, Linker};

use crate::bindings::benchmark_sink;
use crate::bindings::benchmark_sink::vesper::plugin::protocol as wit;
use crate::host_state::{WasmHostState, WasmPluginLogRecord};
use crate::{
    MAX_WASM_PLUGIN_INPUT_BYTES, MAX_WASM_PLUGIN_OUTPUT_BYTES, WASM_PLUGIN_BATCH_TIMEOUT_MILLIS,
    WASM_PLUGIN_FLUSH_TIMEOUT_MILLIS, WasmPluginHostError, WasmPluginRuntime,
};

pub const WASM_PLUGIN_BENCHMARK_BATCH_QUEUE_CAPACITY: usize = 32;
pub const WASM_PLUGIN_BENCHMARK_REPORT_QUEUE_CAPACITY: usize = 32;

/// Immutable owned batch with semantic and WASM transport checks established.
#[derive(Debug)]
struct CheckedBenchmarkBatch(BenchmarkEventBatch);

impl CheckedBenchmarkBatch {
    fn new(batch: BenchmarkEventBatch) -> Result<Self, WasmPluginHostError> {
        validate_benchmark_batch(&batch)?;
        Ok(Self(batch))
    }

    fn copy_from(batch: &BenchmarkEventBatch) -> Result<Self, WasmPluginHostError> {
        // Validate before cloning potentially large borrowed input.
        validate_benchmark_batch(batch)?;
        Ok(Self(batch.clone()))
    }

    fn batch(&self) -> &BenchmarkEventBatch {
        &self.0
    }
}

pub struct WasmBenchmarkSinkSession {
    runtime: WasmPluginRuntime,
    store: Store<WasmHostState>,
    bindings: benchmark_sink::BenchmarkSinkPlugin,
    quarantined: Option<WasmPluginHostError>,
}

impl std::fmt::Debug for WasmBenchmarkSinkSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WasmBenchmarkSinkSession")
            .field("quarantined", &self.quarantined)
            .finish_non_exhaustive()
    }
}

impl WasmBenchmarkSinkSession {
    pub fn from_component_bytes(
        runtime: &WasmPluginRuntime,
        bytes: &[u8],
    ) -> Result<Self, WasmPluginHostError> {
        let component = runtime.compile_component(bytes)?;
        let mut linker = Linker::new(runtime.engine());
        benchmark_sink::BenchmarkSinkPlugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| {
            state
        })
        .map_err(|error| WasmPluginHostError::Instantiation(error.into()))?;
        let mut store =
            runtime.new_store(Duration::from_millis(WASM_PLUGIN_FLUSH_TIMEOUT_MILLIS))?;
        let bindings =
            benchmark_sink::BenchmarkSinkPlugin::instantiate(&mut store, &component, &linker)
                .map_err(|error| WasmPluginHostError::Instantiation(error.into()))?;
        Ok(Self {
            runtime: runtime.clone(),
            store,
            bindings,
            quarantined: None,
        })
    }

    pub fn on_event_batch(
        &mut self,
        batch: &BenchmarkEventBatch,
    ) -> Result<BenchmarkSinkStatus, WasmPluginHostError> {
        self.ensure_available()?;
        validate_benchmark_batch(batch)?;
        self.execute_batch(batch)
    }

    fn on_checked_batch(
        &mut self,
        batch: &CheckedBenchmarkBatch,
    ) -> Result<BenchmarkSinkStatus, WasmPluginHostError> {
        self.ensure_available()?;
        self.execute_batch(batch.batch())
    }

    /// Availability and input validity are established by the entrypoint.
    fn execute_batch(
        &mut self,
        batch: &BenchmarkEventBatch,
    ) -> Result<BenchmarkSinkStatus, WasmPluginHostError> {
        let input = benchmark_batch_to_wit(batch);
        self.runtime.prepare_store(
            &mut self.store,
            Duration::from_millis(WASM_PLUGIN_BATCH_TIMEOUT_MILLIS),
        )?;
        let result = self
            .bindings
            .vesper_plugin_benchmark_sink()
            .call_on_event_batch(&mut self.store, &input)
            .map_err(|error| self.quarantine_execution(error))?;
        let accepted_events = match result {
            Ok(accepted_events) => accepted_events,
            Err(error) => return Err(self.map_plugin_error(error)),
        };
        let status = BenchmarkSinkStatus { accepted_events };
        if let Err(error) = status.validate_for_batch(batch.events.len()) {
            return Err(self.quarantine_protocol(error.to_string()));
        }
        Ok(status)
    }

    pub fn flush(&mut self) -> Result<BenchmarkSinkReport, WasmPluginHostError> {
        self.ensure_available()?;
        self.runtime.prepare_store(
            &mut self.store,
            Duration::from_millis(WASM_PLUGIN_FLUSH_TIMEOUT_MILLIS),
        )?;
        let result = self
            .bindings
            .vesper_plugin_benchmark_sink()
            .call_flush(&mut self.store)
            .map_err(|error| self.quarantine_execution(error))?;
        let report = match result {
            Ok(report) => benchmark_report_from_wit(report)
                .map_err(|message| self.quarantine_protocol(message))?,
            Err(error) => return Err(self.map_plugin_error(error)),
        };
        Ok(report)
    }

    pub fn take_logs(&mut self) -> Vec<WasmPluginLogRecord> {
        self.store.data_mut().take_logs()
    }

    pub fn is_quarantined(&self) -> bool {
        self.quarantined.is_some()
    }

    pub fn quarantine_reason(&self) -> Option<&WasmPluginHostError> {
        self.quarantined.as_ref()
    }

    fn ensure_available(&self) -> Result<(), WasmPluginHostError> {
        if let Some(reason) = &self.quarantined {
            Err(WasmPluginHostError::Quarantined {
                reason: Arc::new(reason.clone()),
            })
        } else {
            Ok(())
        }
    }

    fn quarantine_execution(&mut self, error: wasmtime::Error) -> WasmPluginHostError {
        let failure = WasmPluginHostError::execution(error);
        self.quarantined = Some(failure.clone());
        failure
    }

    fn quarantine_protocol(&mut self, message: String) -> WasmPluginHostError {
        let failure = WasmPluginHostError::ProtocolViolation(message);
        self.quarantined = Some(failure.clone());
        failure
    }

    fn map_plugin_error(&mut self, error: wit::PluginError) -> WasmPluginHostError {
        match error {
            wit::PluginError::InvalidInput(message) => {
                bounded_plugin_error(message, WasmPluginHostError::GuestInvalidInput, self)
            }
            wit::PluginError::Rejected(message) => {
                bounded_plugin_error(message, WasmPluginHostError::Rejected, self)
            }
            wit::PluginError::Failed(message) => {
                bounded_plugin_error(message, WasmPluginHostError::PluginFailed, self)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmBenchmarkBatchEnqueueStatus {
    Enqueued { batch_id: u64 },
    Dropped { batch_id: u64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct WasmBenchmarkSinkBatchReport {
    pub batch_id: u64,
    pub event_count: usize,
    pub result: Result<BenchmarkSinkStatus, WasmPluginHostError>,
    pub logs: Vec<WasmPluginLogRecord>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct WasmBenchmarkSinkQueueReportBatch {
    pub reports: Vec<WasmBenchmarkSinkBatchReport>,
    pub queue_dropped_batches: u64,
    pub queue_dropped_events: u64,
    pub dropped_reports: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WasmBenchmarkSinkFlushReport {
    pub settlement: WasmQueueSettlement,
    pub result: Result<BenchmarkSinkReport, WasmPluginHostError>,
    pub logs: Vec<WasmPluginLogRecord>,
}

#[derive(Debug)]
enum BenchmarkQueueMessage {
    Batch {
        batch_id: u64,
        batch: CheckedBenchmarkBatch,
        completion: Option<SyncSender<WasmBenchmarkSinkBatchReport>>,
    },
    Flush(Arc<BenchmarkFlushReply>),
    Close,
}

#[derive(Debug)]
struct BenchmarkFlushReplyState {
    waiter_active: bool,
    report: Option<WasmBenchmarkSinkFlushReport>,
}

impl Default for BenchmarkFlushReplyState {
    fn default() -> Self {
        Self {
            waiter_active: true,
            report: None,
        }
    }
}

#[derive(Debug, Default)]
struct BenchmarkFlushReply {
    state: Mutex<BenchmarkFlushReplyState>,
    available: Condvar,
}

#[derive(Debug)]
struct BenchmarkQueueState {
    lifecycle: WasmQueueLifecycle,
    messages: VecDeque<BenchmarkQueueMessage>,
    queued_batches: usize,
    accepted_invocations: u64,
    completed_invocations: u64,
    failed_invocations: u64,
    quarantine_reason: Option<WasmPluginHostError>,
    next_batch_id: u64,
    flush_pending: bool,
    completed_timed_out_flush: Option<WasmBenchmarkSinkFlushReport>,
    close_report: Option<WasmBenchmarkSinkFlushReport>,
}

impl Default for BenchmarkQueueState {
    fn default() -> Self {
        Self {
            lifecycle: WasmQueueLifecycle::Open,
            messages: VecDeque::new(),
            queued_batches: 0,
            accepted_invocations: 0,
            completed_invocations: 0,
            failed_invocations: 0,
            quarantine_reason: None,
            next_batch_id: 1,
            flush_pending: false,
            completed_timed_out_flush: None,
            close_report: None,
        }
    }
}

impl BenchmarkQueueState {
    fn settlement(&self, wait_expirations: u64, worker_joined: bool) -> WasmQueueSettlement {
        WasmQueueSettlement {
            lifecycle: self.lifecycle,
            accepted_invocations: self.accepted_invocations,
            completed_invocations: self.completed_invocations,
            failed_invocations: self.failed_invocations,
            wait_expirations,
            pending_invocations: self
                .accepted_invocations
                .saturating_sub(self.completed_invocations),
            worker_joined,
            quarantine_reason: self.quarantine_reason.clone(),
        }
    }

    fn allocate_batch_id(&mut self) -> Result<u64, WasmPluginHostError> {
        if self.next_batch_id == 0 {
            return Err(WasmPluginHostError::Queue(
                crate::WasmQueueError::IdExhausted,
            ));
        }
        let batch_id = self.next_batch_id;
        self.next_batch_id = self.next_batch_id.checked_add(1).unwrap_or(0);
        Ok(batch_id)
    }
}

#[derive(Debug, Default)]
struct BenchmarkQueueReportState {
    reports: VecDeque<WasmBenchmarkSinkBatchReport>,
    queue_dropped_batches: u64,
    queue_dropped_events: u64,
    dropped_reports: u64,
}

#[derive(Debug, Default)]
struct BenchmarkQueueReportSink {
    state: Mutex<BenchmarkQueueReportState>,
}

impl BenchmarkQueueReportSink {
    fn push(&self, report: WasmBenchmarkSinkBatchReport) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.reports.len() >= WASM_PLUGIN_BENCHMARK_REPORT_QUEUE_CAPACITY {
            state.dropped_reports = state.dropped_reports.saturating_add(1);
            return;
        }
        state.reports.push_back(report);
    }

    fn record_queue_drop(&self, event_count: u64) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.queue_dropped_batches = state.queue_dropped_batches.saturating_add(1);
        state.queue_dropped_events = state.queue_dropped_events.saturating_add(event_count);
    }

    fn drain(&self) -> WasmBenchmarkSinkQueueReportBatch {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        WasmBenchmarkSinkQueueReportBatch {
            reports: state.reports.drain(..).collect(),
            queue_dropped_batches: std::mem::take(&mut state.queue_dropped_batches),
            queue_dropped_events: std::mem::take(&mut state.queue_dropped_events),
            dropped_reports: std::mem::take(&mut state.dropped_reports),
        }
    }
}

#[derive(Debug)]
struct WasmBenchmarkSinkQueueShared {
    wait_expirations: Arc<AtomicU64>,
    state: Mutex<BenchmarkQueueState>,
    message_available: Condvar,
    reports: BenchmarkQueueReportSink,
}

impl Default for WasmBenchmarkSinkQueueShared {
    fn default() -> Self {
        Self {
            wait_expirations: Arc::new(AtomicU64::new(0)),
            state: Mutex::new(BenchmarkQueueState::default()),
            message_available: Condvar::new(),
            reports: BenchmarkQueueReportSink::default(),
        }
    }
}

#[derive(Debug)]
struct WasmBenchmarkSinkQueueInner {
    shared: Arc<WasmBenchmarkSinkQueueShared>,
    worker: WasmWorker,
}

impl Drop for WasmBenchmarkSinkQueueInner {
    fn drop(&mut self) {
        request_benchmark_queue_close(&self.shared);
    }
}

#[derive(Debug, Clone)]
pub struct WasmBenchmarkSinkQueue {
    inner: Arc<WasmBenchmarkSinkQueueInner>,
}

impl WasmBenchmarkSinkQueue {
    pub fn from_component_bytes(
        runtime: &WasmPluginRuntime,
        bytes: &[u8],
    ) -> Result<Self, WasmPluginHostError> {
        let session = WasmBenchmarkSinkSession::from_component_bytes(runtime, bytes)?;
        let shared = Arc::new(WasmBenchmarkSinkQueueShared::default());
        let worker_shared = Arc::clone(&shared);
        let worker = std::thread::Builder::new()
            .name("vesper-wasm-benchmark-sink".to_owned())
            .spawn(move || run_benchmark_queue_worker(worker_shared, session))
            .map_err(|error| {
                WasmPluginHostError::Instantiation(wasmtime::Error::from(error).into())
            })?;
        Ok(Self {
            inner: Arc::new(WasmBenchmarkSinkQueueInner {
                shared,
                worker: WasmWorker::new(worker),
            }),
        })
    }

    pub fn enqueue(
        &self,
        batch: BenchmarkEventBatch,
    ) -> Result<WasmBenchmarkBatchEnqueueStatus, WasmPluginHostError> {
        enqueue_benchmark_batch(&self.inner.shared, CheckedBenchmarkBatch::new(batch)?, None)
    }

    /// Accepts one batch without waiting for guest execution.
    pub fn submit(
        &self,
        batch: BenchmarkEventBatch,
    ) -> Result<WasmInvocationTicket<WasmBenchmarkSinkBatchReport>, WasmPluginHostError> {
        self.submit_checked(CheckedBenchmarkBatch::new(batch)?)
    }

    pub(crate) fn submit_borrowed(
        &self,
        batch: &BenchmarkEventBatch,
    ) -> Result<WasmInvocationTicket<WasmBenchmarkSinkBatchReport>, WasmPluginHostError> {
        self.submit_checked(CheckedBenchmarkBatch::copy_from(batch)?)
    }

    fn submit_checked(
        &self,
        batch: CheckedBenchmarkBatch,
    ) -> Result<WasmInvocationTicket<WasmBenchmarkSinkBatchReport>, WasmPluginHostError> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let batch_id = match enqueue_benchmark_batch(&self.inner.shared, batch, Some(sender))? {
            WasmBenchmarkBatchEnqueueStatus::Enqueued { batch_id } => batch_id,
            WasmBenchmarkBatchEnqueueStatus::Dropped { .. } => {
                return Err(WasmPluginHostError::Queue(crate::WasmQueueError::Full));
            }
        };
        Ok(WasmInvocationTicket::new(
            batch_id,
            receiver,
            WasmWaitOperation::BatchResult,
            Arc::clone(&self.inner.shared.wait_expirations),
        ))
    }

    /// Accepts work, then waits. Expiry does not cancel accepted work.
    pub fn submit_and_wait(
        &self,
        batch: BenchmarkEventBatch,
        timeout: Duration,
    ) -> Result<WasmBenchmarkSinkBatchReport, WasmPluginHostError> {
        self.submit(batch)?.wait(timeout)
    }

    pub fn flush(
        &self,
        timeout: Duration,
    ) -> Result<WasmBenchmarkSinkFlushReport, WasmPluginHostError> {
        match begin_benchmark_queue_flush(&self.inner.shared)? {
            BenchmarkQueueFlushRequest::Completed(report) => Ok(report),
            BenchmarkQueueFlushRequest::Pending(reply) => {
                wait_for_benchmark_queue_flush(&reply, timeout)
            }
        }
    }

    pub fn close(
        &self,
        timeout: Duration,
    ) -> Result<WasmBenchmarkSinkFlushReport, WasmPluginHostError> {
        request_benchmark_queue_close(&self.inner.shared);
        let started = Instant::now();
        let mut state = self
            .inner
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while state.close_report.is_none() {
            let Some(remaining) = timeout.checked_sub(started.elapsed()) else {
                return Err(WasmPluginHostError::WaitExpired {
                    operation: crate::WasmWaitOperation::Close,
                    invocation_id: None,
                });
            };
            let waited = self
                .inner
                .shared
                .message_available
                .wait_timeout(state, remaining);
            let (next_state, timeout_result) = match waited {
                Ok(result) => result,
                Err(poisoned) => poisoned.into_inner(),
            };
            state = next_state;
            if timeout_result.timed_out() && state.close_report.is_none() {
                return Err(WasmPluginHostError::WaitExpired {
                    operation: crate::WasmWaitOperation::Close,
                    invocation_id: None,
                });
            }
        }
        let mut report = state
            .close_report
            .clone()
            .ok_or_else(|| WasmPluginHostError::Queue(crate::WasmQueueError::InvalidState))?;
        drop(state);
        self.join_worker(started, timeout)?;
        let mut state = self
            .inner
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.lifecycle = WasmQueueLifecycle::Closed;
        self.inner.shared.message_available.notify_all();
        drop(state);
        report.settlement = self.settlement();
        Ok(report)
    }

    pub fn drain_reports(&self) -> WasmBenchmarkSinkQueueReportBatch {
        self.inner.shared.reports.drain()
    }

    fn join_worker(&self, started: Instant, timeout: Duration) -> Result<(), WasmPluginHostError> {
        self.inner.worker.join(started, timeout)
    }

    /// Returns current accounting, including work isolated by an expired close wait.
    pub fn settlement(&self) -> WasmQueueSettlement {
        let state = self
            .inner
            .shared
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        state.settlement(
            self.inner.shared.wait_expirations.load(Ordering::Relaxed),
            self.inner.worker.is_joined(),
        )
    }
}

fn validate_benchmark_batch(batch: &BenchmarkEventBatch) -> Result<(), WasmPluginHostError> {
    batch
        .validate()
        .map_err(|error| WasmPluginHostError::LocalValidation(error.to_string()))?;
    validate_batch_input_size(batch)
}

fn enqueue_benchmark_batch(
    shared: &WasmBenchmarkSinkQueueShared,
    batch: CheckedBenchmarkBatch,
    completion: Option<SyncSender<WasmBenchmarkSinkBatchReport>>,
) -> Result<WasmBenchmarkBatchEnqueueStatus, WasmPluginHostError> {
    let event_count = u64::try_from(batch.batch().events.len()).unwrap_or(u64::MAX);
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.lifecycle != WasmQueueLifecycle::Open {
        return Err(WasmPluginHostError::Queue(crate::WasmQueueError::Closed));
    }
    let batch_id = state.allocate_batch_id()?;
    if state.queued_batches >= WASM_PLUGIN_BENCHMARK_BATCH_QUEUE_CAPACITY {
        shared.reports.record_queue_drop(event_count);
        return Ok(WasmBenchmarkBatchEnqueueStatus::Dropped { batch_id });
    }
    state.accepted_invocations = state.accepted_invocations.saturating_add(1);
    state.queued_batches += 1;
    state.messages.push_back(BenchmarkQueueMessage::Batch {
        batch_id,
        batch,
        completion,
    });
    shared.message_available.notify_one();
    Ok(WasmBenchmarkBatchEnqueueStatus::Enqueued { batch_id })
}

enum BenchmarkQueueFlushRequest {
    Completed(WasmBenchmarkSinkFlushReport),
    Pending(Arc<BenchmarkFlushReply>),
}

fn begin_benchmark_queue_flush(
    shared: &WasmBenchmarkSinkQueueShared,
) -> Result<BenchmarkQueueFlushRequest, WasmPluginHostError> {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.lifecycle != WasmQueueLifecycle::Open {
        return Err(WasmPluginHostError::Queue(crate::WasmQueueError::Closed));
    }
    if let Some(report) = state.completed_timed_out_flush.take() {
        return Ok(BenchmarkQueueFlushRequest::Completed(report));
    }
    if state.flush_pending {
        return Err(WasmPluginHostError::Queue(
            crate::WasmQueueError::FlushPending,
        ));
    }
    let reply = Arc::new(BenchmarkFlushReply::default());
    state.flush_pending = true;
    state
        .messages
        .push_back(BenchmarkQueueMessage::Flush(Arc::clone(&reply)));
    shared.message_available.notify_one();
    Ok(BenchmarkQueueFlushRequest::Pending(reply))
}

fn wait_for_benchmark_queue_flush(
    reply: &BenchmarkFlushReply,
    timeout: Duration,
) -> Result<WasmBenchmarkSinkFlushReport, WasmPluginHostError> {
    let state = reply
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let waited = reply
        .available
        .wait_timeout_while(state, timeout, |state| state.report.is_none());
    let (mut state, _) = match waited {
        Ok(result) => result,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(report) = state.report.take() {
        return Ok(report);
    }
    state.waiter_active = false;
    Err(WasmPluginHostError::WaitExpired {
        operation: crate::WasmWaitOperation::Flush,
        invocation_id: None,
    })
}

fn request_benchmark_queue_close(shared: &WasmBenchmarkSinkQueueShared) {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.lifecycle == WasmQueueLifecycle::Open {
        state.lifecycle = WasmQueueLifecycle::Closing;
        state.messages.push_back(BenchmarkQueueMessage::Close);
        shared.message_available.notify_one();
    }
}

fn take_benchmark_queue_message(
    shared: &WasmBenchmarkSinkQueueShared,
) -> Option<BenchmarkQueueMessage> {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    loop {
        if let Some(message) = state.messages.pop_front() {
            if matches!(message, BenchmarkQueueMessage::Batch { .. }) {
                state.queued_batches = state.queued_batches.saturating_sub(1);
            }
            return Some(message);
        }
        if state.lifecycle == WasmQueueLifecycle::Closed {
            return None;
        }
        state = shared
            .message_available
            .wait(state)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
}

struct ActiveBenchmarkBatch {
    batch_id: u64,
    event_count: usize,
    completion: Option<SyncSender<WasmBenchmarkSinkBatchReport>>,
}

fn publish_benchmark_batch_report(
    shared: &WasmBenchmarkSinkQueueShared,
    report: WasmBenchmarkSinkBatchReport,
    completion: Option<SyncSender<WasmBenchmarkSinkBatchReport>>,
) {
    {
        let mut state = shared.state.lock().unwrap_or_else(|p| p.into_inner());
        state.completed_invocations = state.completed_invocations.saturating_add(1);
        if report.result.is_err() {
            state.failed_invocations = state.failed_invocations.saturating_add(1);
        }
    }
    if let Some(completion) = completion {
        shared.reports.push(report.clone());
        let _ = completion.try_send(report);
    } else {
        shared.reports.push(report);
    }
}

fn run_benchmark_queue_worker(
    shared: Arc<WasmBenchmarkSinkQueueShared>,
    mut session: WasmBenchmarkSinkSession,
) {
    let mut active_batch = None;
    let worker_result = catch_unwind(AssertUnwindSafe(|| {
        loop {
            let Some(message) = take_benchmark_queue_message(&shared) else {
                break WasmBenchmarkSinkFlushReport {
                    settlement: WasmQueueSettlement::default(),
                    result: Err(WasmPluginHostError::Queue(
                        crate::WasmQueueError::WorkerStopped,
                    )),
                    logs: session.take_logs(),
                };
            };
            match message {
                BenchmarkQueueMessage::Batch {
                    batch_id,
                    batch,
                    completion,
                } => {
                    let event_count = batch.batch().events.len();
                    active_batch = Some(ActiveBenchmarkBatch {
                        batch_id,
                        event_count,
                        completion,
                    });
                    let result = call_benchmark_batch(&mut session, &batch);
                    {
                        let mut state = shared.state.lock().unwrap_or_else(|p| p.into_inner());
                        state.quarantine_reason = session.quarantined.clone();
                    }
                    let report = WasmBenchmarkSinkBatchReport {
                        batch_id,
                        event_count,
                        result,
                        logs: session.take_logs(),
                    };
                    let completion = active_batch.take().and_then(|active| active.completion);
                    publish_benchmark_batch_report(&shared, report, completion);
                }
                BenchmarkQueueMessage::Flush(reply) => {
                    let report = flush_benchmark_session(&shared, &mut session);
                    complete_benchmark_queue_flush(&shared, &reply, report);
                }
                BenchmarkQueueMessage::Close => {
                    break flush_benchmark_session(&shared, &mut session);
                }
            }
        }
    }));
    let report = match worker_result {
        Ok(report) => report,
        Err(_) => {
            let reason = WasmPluginHostError::WorkerPanic("benchmark queue worker".to_owned());
            session.quarantined = Some(reason.clone());
            fail_benchmark_queue_after_worker_panic(&shared, active_batch.take(), &reason);
            WasmBenchmarkSinkFlushReport {
                settlement: WasmQueueSettlement::default(),
                result: Err(reason),
                logs: session.take_logs(),
            }
        }
    };
    drop(session);
    publish_benchmark_queue_close_report(&shared, report);
}

fn complete_benchmark_queue_flush(
    shared: &WasmBenchmarkSinkQueueShared,
    reply: &BenchmarkFlushReply,
    mut report: WasmBenchmarkSinkFlushReport,
) {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut reply_state = reply
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    report.settlement = state.settlement(shared.wait_expirations.load(Ordering::Relaxed), false);
    if reply_state.waiter_active {
        reply_state.report = Some(report);
    } else {
        state.completed_timed_out_flush = Some(report);
    }
    state.flush_pending = false;
    shared.message_available.notify_all();
    reply.available.notify_all();
}

fn call_benchmark_batch(
    session: &mut WasmBenchmarkSinkSession,
    batch: &CheckedBenchmarkBatch,
) -> Result<BenchmarkSinkStatus, WasmPluginHostError> {
    catch_unwind(AssertUnwindSafe(|| session.on_checked_batch(batch))).unwrap_or_else(|_| {
        let failure = WasmPluginHostError::WorkerPanic("benchmark batch invocation".to_owned());
        session.quarantined = Some(failure.clone());
        Err(failure)
    })
}

fn flush_benchmark_session(
    shared: &WasmBenchmarkSinkQueueShared,
    session: &mut WasmBenchmarkSinkSession,
) -> WasmBenchmarkSinkFlushReport {
    let result = catch_unwind(AssertUnwindSafe(|| session.flush())).unwrap_or_else(|_| {
        let failure = WasmPluginHostError::WorkerPanic("benchmark flush invocation".to_owned());
        session.quarantined = Some(failure.clone());
        Err(failure)
    });
    let settlement = {
        let mut state = shared.state.lock().unwrap_or_else(|p| p.into_inner());
        state.quarantine_reason = session.quarantined.clone();
        state.settlement(shared.wait_expirations.load(Ordering::Relaxed), false)
    };
    WasmBenchmarkSinkFlushReport {
        settlement,
        result,
        logs: session.take_logs(),
    }
}

fn fail_benchmark_queue_after_worker_panic(
    shared: &WasmBenchmarkSinkQueueShared,
    active_batch: Option<ActiveBenchmarkBatch>,
    reason: &WasmPluginHostError,
) {
    let pending = {
        let mut state = shared.state.lock().unwrap_or_else(|p| p.into_inner());
        state.lifecycle = WasmQueueLifecycle::Closing;
        state.quarantine_reason = Some(reason.clone());
        state.queued_batches = 0;
        state.flush_pending = false;
        state.messages.drain(..).collect::<Vec<_>>()
    };
    if let Some(ActiveBenchmarkBatch {
        batch_id,
        event_count,
        completion,
    }) = active_batch
    {
        publish_benchmark_batch_report(
            shared,
            WasmBenchmarkSinkBatchReport {
                batch_id,
                event_count,
                result: Err(reason.clone()),
                logs: Vec::new(),
            },
            completion,
        );
    }
    for message in pending {
        match message {
            BenchmarkQueueMessage::Batch {
                batch_id,
                batch,
                completion,
            } => {
                let report = WasmBenchmarkSinkBatchReport {
                    batch_id,
                    event_count: batch.batch().events.len(),
                    result: Err(WasmPluginHostError::Quarantined {
                        reason: Arc::new(reason.clone()),
                    }),
                    logs: Vec::new(),
                };
                publish_benchmark_batch_report(shared, report, completion);
            }
            BenchmarkQueueMessage::Flush(reply) => {
                complete_benchmark_queue_flush(
                    shared,
                    &reply,
                    WasmBenchmarkSinkFlushReport {
                        settlement: WasmQueueSettlement::default(),
                        result: Err(reason.clone()),
                        logs: Vec::new(),
                    },
                );
            }
            BenchmarkQueueMessage::Close => {}
        }
    }
    shared.message_available.notify_all();
}

fn publish_benchmark_queue_close_report(
    shared: &WasmBenchmarkSinkQueueShared,
    mut report: WasmBenchmarkSinkFlushReport,
) {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.close_report.is_none() {
        state.lifecycle = WasmQueueLifecycle::Closed;
        report.settlement =
            state.settlement(shared.wait_expirations.load(Ordering::Relaxed), false);
        state.close_report = Some(report);
        state.flush_pending = false;
        shared.message_available.notify_all();
    }
}

fn bounded_plugin_error(
    message: String,
    map: fn(String) -> WasmPluginHostError,
    session: &mut WasmBenchmarkSinkSession,
) -> WasmPluginHostError {
    if message.is_empty() || message.len() > MAX_PLUGIN_ERROR_MESSAGE_BYTES {
        return session.quarantine_protocol(format!(
            "plugin error must contain 1 to {MAX_PLUGIN_ERROR_MESSAGE_BYTES} UTF-8 bytes"
        ));
    }
    map(message)
}

fn validate_batch_input_size(batch: &BenchmarkEventBatch) -> Result<(), WasmPluginHostError> {
    let mut size = 0_usize;
    for event in &batch.events {
        for value in [
            Some(event.run_id.as_str()),
            Some(event.session_id.as_str()),
            Some(event.platform.as_str()),
            event.source_protocol.as_deref(),
            Some(event.event_name.as_str()),
            event.thread.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            size = size.checked_add(value.len()).ok_or_else(|| {
                WasmPluginHostError::LocalValidation("benchmark input size overflow".to_owned())
            })?;
        }
        for (key, value) in &event.attributes {
            size = size
                .checked_add(key.len())
                .and_then(|size| size.checked_add(value.len()))
                .ok_or_else(|| {
                    WasmPluginHostError::LocalValidation("benchmark input size overflow".to_owned())
                })?;
        }
        if size > MAX_WASM_PLUGIN_INPUT_BYTES {
            return Err(WasmPluginHostError::LocalValidation(format!(
                "benchmark batch exceeds the {MAX_WASM_PLUGIN_INPUT_BYTES}-byte WASM input limit"
            )));
        }
    }
    Ok(())
}

fn benchmark_batch_to_wit(batch: &BenchmarkEventBatch) -> wit::BenchmarkBatch {
    wit::BenchmarkBatch {
        events: batch.events.iter().map(benchmark_event_to_wit).collect(),
    }
}

fn benchmark_event_to_wit(event: &BenchmarkEvent) -> wit::BenchmarkEvent {
    wit::BenchmarkEvent {
        run_id: event.run_id.clone(),
        session_id: event.session_id.clone(),
        platform: event.platform.clone(),
        protocol: event.source_protocol.clone(),
        event_name: event.event_name.clone(),
        timestamp_ns: event.timestamp_ns,
        elapsed_ns: event.elapsed_ns,
        thread: event.thread.clone(),
        attributes: event
            .attributes
            .iter()
            .map(|(key, value)| wit::Attribute {
                key: key.clone(),
                value: value.clone(),
            })
            .collect(),
    }
}

fn benchmark_report_from_wit(report: wit::BenchmarkReport) -> Result<BenchmarkSinkReport, String> {
    if report.measurements.len() > MAX_PLUGIN_MEASUREMENTS {
        return Err(format!(
            "benchmark report exceeds the {MAX_PLUGIN_MEASUREMENTS}-measurement protocol limit"
        ));
    }
    if report.diagnostics.len() > MAX_PLUGIN_DIAGNOSTICS {
        return Err(format!(
            "benchmark report exceeds the {MAX_PLUGIN_DIAGNOSTICS}-diagnostic protocol limit"
        ));
    }
    if report.threshold_violations.len() > MAX_BENCHMARK_THRESHOLD_VIOLATIONS {
        return Err(format!(
            "benchmark report exceeds the {MAX_BENCHMARK_THRESHOLD_VIOLATIONS}-threshold-violation protocol limit"
        ));
    }
    validate_benchmark_output_size(&report)?;
    let report = BenchmarkSinkReport {
        accepted_events: report.accepted_events,
        dropped_events: report.dropped_events,
        measurements: report
            .measurements
            .into_iter()
            .map(measurement_from_wit)
            .collect::<Result<_, _>>()?,
        threshold_violations: report
            .threshold_violations
            .into_iter()
            .map(|violation| BenchmarkThresholdViolation {
                measurement: violation.measurement,
                actual: violation.actual,
                threshold: violation.threshold,
                comparison: violation.comparison,
            })
            .collect(),
        diagnostics: report
            .diagnostics
            .into_iter()
            .map(diagnostic_from_wit)
            .collect::<Result<_, _>>()?,
    };
    report.validate().map_err(|error| error.to_string())?;
    Ok(report)
}

fn validate_benchmark_output_size(report: &wit::BenchmarkReport) -> Result<(), String> {
    let mut size = 64_usize;
    add_benchmark_output_entries(&mut size, report.measurements.len(), 32)?;
    add_benchmark_output_entries(&mut size, report.threshold_violations.len(), 48)?;
    add_benchmark_output_entries(&mut size, report.diagnostics.len(), 32)?;
    for measurement in &report.measurements {
        add_benchmark_output_bytes(&mut size, measurement.name.len())?;
        add_benchmark_output_bytes(&mut size, measurement.unit.len())?;
        add_benchmark_wit_attribute_bytes(&mut size, &measurement.attributes)?;
    }
    for violation in &report.threshold_violations {
        add_benchmark_output_bytes(&mut size, violation.measurement.len())?;
        add_benchmark_output_bytes(&mut size, violation.comparison.len())?;
    }
    for diagnostic in &report.diagnostics {
        add_benchmark_output_bytes(&mut size, diagnostic.code.len())?;
        add_benchmark_output_bytes(&mut size, diagnostic.message.len())?;
        add_benchmark_wit_attribute_bytes(&mut size, &diagnostic.attributes)?;
    }
    if size > MAX_WASM_PLUGIN_OUTPUT_BYTES {
        return Err(format!(
            "benchmark output exceeds the {MAX_WASM_PLUGIN_OUTPUT_BYTES}-byte WASM output limit"
        ));
    }
    Ok(())
}

fn add_benchmark_output_entries(
    size: &mut usize,
    count: usize,
    entry_bytes: usize,
) -> Result<(), String> {
    let added = count
        .checked_mul(entry_bytes)
        .ok_or_else(|| "benchmark output size overflow".to_owned())?;
    add_benchmark_output_bytes(size, added)
}

fn add_benchmark_output_bytes(size: &mut usize, added: usize) -> Result<(), String> {
    *size = size
        .checked_add(added)
        .ok_or_else(|| "benchmark output size overflow".to_owned())?;
    Ok(())
}

fn add_benchmark_wit_attribute_bytes(
    size: &mut usize,
    attributes: &[wit::Attribute],
) -> Result<(), String> {
    add_benchmark_output_entries(size, attributes.len(), 16)?;
    for attribute in attributes {
        add_benchmark_output_bytes(size, attribute.key.len())?;
        add_benchmark_output_bytes(size, attribute.value.len())?;
    }
    Ok(())
}

fn measurement_from_wit(measurement: wit::Measurement) -> Result<PluginMeasurement, String> {
    Ok(PluginMeasurement {
        name: measurement.name,
        value: measurement.value,
        unit: measurement.unit,
        attributes: attributes_from_wit(measurement.attributes)?,
    })
}

fn diagnostic_from_wit(diagnostic: wit::Diagnostic) -> Result<PluginDiagnostic, String> {
    Ok(PluginDiagnostic {
        code: diagnostic.code,
        severity: match diagnostic.severity {
            wit::DiagnosticSeverity::Info => PluginDiagnosticSeverity::Info,
            wit::DiagnosticSeverity::Warning => PluginDiagnosticSeverity::Warning,
            wit::DiagnosticSeverity::Error => PluginDiagnosticSeverity::Error,
        },
        message: diagnostic.message,
        attributes: attributes_from_wit(diagnostic.attributes)?,
    })
}

fn attributes_from_wit(
    attributes: Vec<wit::Attribute>,
) -> Result<BTreeMap<String, String>, String> {
    crate::attributes::decode_attributes(
        attributes
            .into_iter()
            .map(|attribute| (attribute.key, attribute.value)),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Barrier, mpsc};

    use super::*;
    use player_plugin::{MAX_PLUGIN_ATTRIBUTE_VALUE_BYTES, MAX_PLUGIN_ATTRIBUTES};

    fn queue_test_event(timestamp_ns: u64) -> BenchmarkEvent {
        BenchmarkEvent {
            run_id: "run".to_owned(),
            session_id: "session".to_owned(),
            platform: "test".to_owned(),
            source_protocol: None,
            event_name: "benchmark.event".to_owned(),
            timestamp_ns,
            elapsed_ns: timestamp_ns,
            thread: None,
            attributes: BTreeMap::new(),
        }
    }

    fn queue_test_batch(event_count: usize) -> BenchmarkEventBatch {
        BenchmarkEventBatch {
            events: (0..event_count)
                .map(|index| queue_test_event(u64::try_from(index).expect("test event index")))
                .collect(),
        }
    }

    fn queue_test_batch_report(batch_id: u64) -> WasmBenchmarkSinkBatchReport {
        WasmBenchmarkSinkBatchReport {
            batch_id,
            event_count: 1,
            result: Ok(BenchmarkSinkStatus { accepted_events: 1 }),
            logs: Vec::new(),
        }
    }

    fn queue_test_checked_batch(event_count: usize) -> CheckedBenchmarkBatch {
        CheckedBenchmarkBatch::new(queue_test_batch(event_count)).expect("checked batch")
    }

    fn queue_test_flush_report() -> WasmBenchmarkSinkFlushReport {
        WasmBenchmarkSinkFlushReport {
            settlement: WasmQueueSettlement::default(),
            result: Ok(BenchmarkSinkReport::default()),
            logs: Vec::new(),
        }
    }

    fn queue_test_closed_report() -> WasmBenchmarkSinkFlushReport {
        let mut report = queue_test_flush_report();
        report.settlement.lifecycle = WasmQueueLifecycle::Closed;
        report.settlement.worker_joined = true;
        report
    }

    #[test]
    fn component_without_benchmark_exports_is_rejected() {
        let runtime = WasmPluginRuntime::new().expect("WASM runtime");
        let bytes = wat::parse_str("(component)").expect("empty component");

        assert!(matches!(
            WasmBenchmarkSinkSession::from_component_bytes(&runtime, &bytes),
            Err(WasmPluginHostError::Instantiation(message))
                if message.to_string().contains("benchmark-sink")
        ));
    }

    #[test]
    fn duplicate_wit_attributes_are_rejected_instead_of_overwritten() {
        let attributes = vec![
            wit::Attribute {
                key: "codec".to_owned(),
                value: "h264".to_owned(),
            },
            wit::Attribute {
                key: "codec".to_owned(),
                value: "hevc".to_owned(),
            },
        ];

        assert!(matches!(
            attributes_from_wit(attributes),
            Err(message) if message.contains("duplicate attribute key 'codec'")
        ));
    }

    #[test]
    fn threshold_violations_are_bounded_before_domain_conversion() {
        let report = wit::BenchmarkReport {
            accepted_events: 0,
            dropped_events: 0,
            measurements: Vec::new(),
            threshold_violations: (0..=MAX_BENCHMARK_THRESHOLD_VIOLATIONS)
                .map(|_| wit::ThresholdViolation {
                    measurement: "latency".to_owned(),
                    actual: 2.0,
                    threshold: 1.0,
                    comparison: "greater-than".to_owned(),
                })
                .collect(),
            diagnostics: Vec::new(),
        };

        assert!(matches!(
            benchmark_report_from_wit(report),
            Err(message) if message.contains("threshold-violation protocol limit")
        ));
    }

    #[test]
    fn oversized_benchmark_output_is_rejected_before_domain_conversion() {
        let report = wit::BenchmarkReport {
            accepted_events: 0,
            dropped_events: 0,
            measurements: (0..MAX_PLUGIN_MEASUREMENTS)
                .map(|measurement_index| wit::Measurement {
                    name: format!("measurement-{measurement_index}"),
                    value: 1.0,
                    unit: "nanoseconds".to_owned(),
                    attributes: (0..MAX_PLUGIN_ATTRIBUTES)
                        .map(|attribute_index| wit::Attribute {
                            key: format!(
                                "{measurement_index:03}-{attribute_index:02}-{}",
                                "k".repeat(56)
                            ),
                            value: "v".repeat(MAX_PLUGIN_ATTRIBUTE_VALUE_BYTES),
                        })
                        .collect(),
                })
                .collect(),
            threshold_violations: Vec::new(),
            diagnostics: Vec::new(),
        };

        assert!(matches!(
            benchmark_report_from_wit(report),
            Err(message) if message.contains("WASM output limit")
        ));
    }

    #[test]
    fn full_batch_queue_drops_newest_with_a_stable_id_and_exact_counts() {
        let shared = WasmBenchmarkSinkQueueShared::default();

        for expected_id in 1..=WASM_PLUGIN_BENCHMARK_BATCH_QUEUE_CAPACITY {
            assert_eq!(
                enqueue_benchmark_batch(&shared, queue_test_checked_batch(2), None)
                    .expect("accepted batch"),
                WasmBenchmarkBatchEnqueueStatus::Enqueued {
                    batch_id: u64::try_from(expected_id).expect("batch id"),
                }
            );
        }
        assert_eq!(
            enqueue_benchmark_batch(&shared, queue_test_checked_batch(3), None)
                .expect("dropped batch"),
            WasmBenchmarkBatchEnqueueStatus::Dropped { batch_id: 33 }
        );

        let state = shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(
            state.queued_batches,
            WASM_PLUGIN_BENCHMARK_BATCH_QUEUE_CAPACITY
        );
        let retained_ids = state
            .messages
            .iter()
            .filter_map(|message| match message {
                BenchmarkQueueMessage::Batch { batch_id, .. } => Some(*batch_id),
                BenchmarkQueueMessage::Flush(_) | BenchmarkQueueMessage::Close => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(retained_ids, (1..=32).collect::<Vec<_>>());
        drop(state);
        let dropped = shared.reports.drain();
        assert!(dropped.reports.is_empty());
        assert_eq!(dropped.queue_dropped_batches, 1);
        assert_eq!(dropped.queue_dropped_events, 3);
        assert_eq!(dropped.dropped_reports, 0);
    }

    #[test]
    fn full_batch_queue_still_accepts_fifo_flush_and_close_controls() {
        let shared = WasmBenchmarkSinkQueueShared::default();
        for _ in 0..WASM_PLUGIN_BENCHMARK_BATCH_QUEUE_CAPACITY {
            enqueue_benchmark_batch(&shared, queue_test_checked_batch(1), None)
                .expect("accepted batch");
        }
        let _flush_reply = match begin_benchmark_queue_flush(&shared).expect("flush control") {
            BenchmarkQueueFlushRequest::Pending(reply) => reply,
            BenchmarkQueueFlushRequest::Completed(_) => panic!("unexpected completed flush"),
        };
        request_benchmark_queue_close(&shared);

        let state = shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(state.lifecycle, WasmQueueLifecycle::Closing);
        assert_eq!(
            state.queued_batches,
            WASM_PLUGIN_BENCHMARK_BATCH_QUEUE_CAPACITY
        );
        assert_eq!(
            state.messages.len(),
            WASM_PLUGIN_BENCHMARK_BATCH_QUEUE_CAPACITY + 2
        );
        assert!(matches!(
            state
                .messages
                .get(WASM_PLUGIN_BENCHMARK_BATCH_QUEUE_CAPACITY),
            Some(BenchmarkQueueMessage::Flush(_))
        ));
        assert!(matches!(
            state
                .messages
                .get(WASM_PLUGIN_BENCHMARK_BATCH_QUEUE_CAPACITY + 1),
            Some(BenchmarkQueueMessage::Close)
        ));
    }

    #[test]
    fn report_queue_drops_the_newest_report_and_resets_its_counter() {
        let sink = BenchmarkQueueReportSink::default();
        for batch_id in 1..=33 {
            sink.push(queue_test_batch_report(batch_id));
        }

        let batch = sink.drain();
        assert_eq!(
            batch.reports.len(),
            WASM_PLUGIN_BENCHMARK_REPORT_QUEUE_CAPACITY
        );
        assert_eq!(batch.reports.first().map(|report| report.batch_id), Some(1));
        assert_eq!(batch.reports.last().map(|report| report.batch_id), Some(32));
        assert_eq!(batch.queue_dropped_batches, 0);
        assert_eq!(batch.queue_dropped_events, 0);
        assert_eq!(batch.dropped_reports, 1);
        assert_eq!(sink.drain(), WasmBenchmarkSinkQueueReportBatch::default());
    }

    #[test]
    fn flush_pending_is_cleared_before_the_reply_is_delivered() {
        let shared = Arc::new(WasmBenchmarkSinkQueueShared::default());
        shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .flush_pending = true;
        let reply = Arc::new(BenchmarkFlushReply::default());
        let worker_reply = Arc::clone(&reply);
        let worker_shared = Arc::clone(&shared);
        let worker = std::thread::spawn(move || {
            complete_benchmark_queue_flush(
                &worker_shared,
                &worker_reply,
                queue_test_flush_report(),
            );
        });

        assert_eq!(
            wait_for_benchmark_queue_flush(&reply, Duration::from_secs(1)),
            Ok(queue_test_flush_report())
        );
        assert!(
            !shared
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .flush_pending
        );
        worker.join().expect("flush completion worker");
    }

    #[test]
    fn host_flush_timeout_does_not_poison_the_queue_or_guest_result() {
        let shared = Arc::new(WasmBenchmarkSinkQueueShared::default());
        let worker_shared = Arc::clone(&shared);
        let release_first_flush = Arc::new((Mutex::new(false), Condvar::new()));
        let worker_release = Arc::clone(&release_first_flush);
        let flush_calls = Arc::new(AtomicUsize::new(0));
        let worker_flush_calls = Arc::clone(&flush_calls);
        let worker = std::thread::spawn(move || {
            let mut is_first_flush = true;
            while let Some(message) = take_benchmark_queue_message(&worker_shared) {
                match message {
                    BenchmarkQueueMessage::Batch { .. } => {
                        panic!("test worker does not accept batch messages")
                    }
                    BenchmarkQueueMessage::Flush(reply) => {
                        worker_flush_calls.fetch_add(1, Ordering::Relaxed);
                        if is_first_flush {
                            let (released, release_changed) = &*worker_release;
                            let released = released
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            let _released = release_changed
                                .wait_while(released, |released| !*released)
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            is_first_flush = false;
                        }
                        complete_benchmark_queue_flush(
                            &worker_shared,
                            &reply,
                            queue_test_flush_report(),
                        );
                    }
                    BenchmarkQueueMessage::Close => {
                        publish_benchmark_queue_close_report(
                            &worker_shared,
                            queue_test_flush_report(),
                        );
                        return;
                    }
                }
            }
        });
        let queue = WasmBenchmarkSinkQueue {
            inner: Arc::new(WasmBenchmarkSinkQueueInner {
                shared: Arc::clone(&shared),
                worker: WasmWorker::new(worker),
            }),
        };

        assert_eq!(
            queue.flush(Duration::from_millis(10)),
            Err(WasmPluginHostError::WaitExpired {
                operation: crate::WasmWaitOperation::Flush,
                invocation_id: None
            })
        );
        assert_eq!(
            shared
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .lifecycle,
            WasmQueueLifecycle::Open
        );

        let (released, release_changed) = &*release_first_flush;
        *released
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
        release_changed.notify_all();
        let state = shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let waited =
            shared
                .message_available
                .wait_timeout_while(state, Duration::from_secs(1), |state| state.flush_pending);
        let (state, timeout_result) = match waited {
            Ok(result) => result,
            Err(poisoned) => poisoned.into_inner(),
        };
        assert!(!timeout_result.timed_out());
        assert!(!state.flush_pending);
        drop(state);

        assert_eq!(
            queue.flush(Duration::from_secs(1)),
            Ok(queue_test_flush_report())
        );
        assert_eq!(flush_calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            queue.close(Duration::from_secs(1)),
            Ok(queue_test_closed_report())
        );
    }

    #[test]
    fn worker_panic_reports_the_active_batch_and_quarantines_pending_batches() {
        let shared = WasmBenchmarkSinkQueueShared::default();
        let (first_tx, first_rx) = mpsc::sync_channel(1);
        let (second_tx, second_rx) = mpsc::sync_channel(1);
        enqueue_benchmark_batch(&shared, queue_test_checked_batch(1), Some(first_tx))
            .expect("first batch");
        enqueue_benchmark_batch(&shared, queue_test_checked_batch(2), Some(second_tx))
            .expect("second batch");
        let active = match take_benchmark_queue_message(&shared).expect("active batch") {
            BenchmarkQueueMessage::Batch {
                batch_id,
                batch,
                completion,
            } => ActiveBenchmarkBatch {
                batch_id,
                event_count: batch.batch().events.len(),
                completion,
            },
            BenchmarkQueueMessage::Flush(_) | BenchmarkQueueMessage::Close => {
                panic!("expected a batch message")
            }
        };

        let reason = WasmPluginHostError::WorkerPanic("benchmark queue worker".to_owned());
        fail_benchmark_queue_after_worker_panic(&shared, Some(active), &reason);

        assert_eq!(
            first_rx
                .recv_timeout(Duration::ZERO)
                .expect("active receipt")
                .batch_id,
            1
        );
        assert_eq!(
            second_rx
                .recv_timeout(Duration::ZERO)
                .expect("pending receipt")
                .batch_id,
            2
        );
        let reports = shared.reports.drain().reports;
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].batch_id, 1);
        assert!(matches!(
            reports[0].result,
            Err(WasmPluginHostError::WorkerPanic(_))
        ));
        assert_eq!(reports[1].batch_id, 2);
        assert!(matches!(
            reports[1].result,
            Err(WasmPluginHostError::Quarantined { .. })
        ));
        let state = shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(state.lifecycle, WasmQueueLifecycle::Closing);
        assert_eq!(state.queued_batches, 0);
        assert!(state.messages.is_empty());
        assert_eq!(state.completed_invocations, state.accepted_invocations);
        assert_eq!(state.failed_invocations, 2);
        assert_eq!(state.quarantine_reason, Some(reason));
    }

    #[test]
    fn closed_guest_state_does_not_claim_a_joined_worker() {
        let shared = Arc::new(WasmBenchmarkSinkQueueShared::default());
        let worker_shared = Arc::clone(&shared);
        let report_published = Arc::new(Barrier::new(2));
        let worker_report_published = Arc::clone(&report_published);
        let release_worker = Arc::new((Mutex::new(false), Condvar::new()));
        let worker_release = Arc::clone(&release_worker);
        let worker = std::thread::spawn(move || {
            assert!(matches!(
                take_benchmark_queue_message(&worker_shared),
                Some(BenchmarkQueueMessage::Close)
            ));
            publish_benchmark_queue_close_report(&worker_shared, queue_test_flush_report());
            worker_report_published.wait();
            let (released, release_changed) = &*worker_release;
            let released = released
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let _released = release_changed
                .wait_while(released, |released| !*released)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        });
        let queue = WasmBenchmarkSinkQueue {
            inner: Arc::new(WasmBenchmarkSinkQueueInner {
                shared: Arc::clone(&shared),
                worker: WasmWorker::new(worker),
            }),
        };

        request_benchmark_queue_close(&shared);
        report_published.wait();
        assert!(matches!(
            queue.close(Duration::from_millis(10)),
            Err(WasmPluginHostError::WaitExpired {
                operation: crate::WasmWaitOperation::WorkerJoin,
                ..
            })
        ));
        assert_eq!(
            shared
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .lifecycle,
            WasmQueueLifecycle::Closed
        );
        assert!(!queue.settlement().worker_joined);

        let (released, release_changed) = &*release_worker;
        *released
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
        release_changed.notify_all();

        let expected = queue_test_closed_report();
        assert_eq!(queue.close(Duration::from_secs(1)), Ok(expected.clone()));
        assert_eq!(queue.close(Duration::ZERO), Ok(expected));
        assert_eq!(
            shared
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .lifecycle,
            WasmQueueLifecycle::Closed
        );
    }

    #[test]
    fn concurrent_close_callers_wait_for_the_same_worker_join() {
        let shared = Arc::new(WasmBenchmarkSinkQueueShared::default());
        let worker_shared = Arc::clone(&shared);
        let worker_report_published = Arc::new(Barrier::new(2));
        let published = Arc::clone(&worker_report_published);
        let release_worker = Arc::new((Mutex::new(false), Condvar::new()));
        let worker_release = Arc::clone(&release_worker);
        let worker = std::thread::spawn(move || {
            assert!(matches!(
                take_benchmark_queue_message(&worker_shared),
                Some(BenchmarkQueueMessage::Close)
            ));
            publish_benchmark_queue_close_report(&worker_shared, queue_test_flush_report());
            published.wait();
            let (released, release_changed) = &*worker_release;
            let released = released
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let _released = release_changed
                .wait_while(released, |released| !*released)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        });
        let queue = WasmBenchmarkSinkQueue {
            inner: Arc::new(WasmBenchmarkSinkQueueInner {
                shared,
                worker: WasmWorker::new(worker),
            }),
        };
        let callers_ready = Arc::new(Barrier::new(3));
        let (result_tx, result_rx) = mpsc::channel();
        let callers = (0..2)
            .map(|_| {
                let queue = queue.clone();
                let callers_ready = Arc::clone(&callers_ready);
                let result_tx = result_tx.clone();
                std::thread::spawn(move || {
                    callers_ready.wait();
                    let _ = result_tx.send(queue.close(Duration::from_secs(1)));
                })
            })
            .collect::<Vec<_>>();
        drop(result_tx);

        callers_ready.wait();
        worker_report_published.wait();
        assert_eq!(
            result_rx.recv_timeout(Duration::from_millis(20)),
            Err(mpsc::RecvTimeoutError::Timeout)
        );

        let (released, release_changed) = &*release_worker;
        *released
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
        release_changed.notify_all();

        let expected = Ok(queue_test_closed_report());
        assert_eq!(
            result_rx.recv_timeout(Duration::from_secs(1)),
            Ok(expected.clone())
        );
        assert_eq!(result_rx.recv_timeout(Duration::from_secs(1)), Ok(expected));
        for caller in callers {
            caller.join().expect("close caller");
        }
    }

    #[test]
    fn expired_batch_wait_keeps_the_guest_work_and_correlated_report() {
        let shared = Arc::new(WasmBenchmarkSinkQueueShared::default());
        let worker_shared = Arc::clone(&shared);
        let (release, released) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let mut accepted_events = 0;
            while let Some(message) = take_benchmark_queue_message(&worker_shared) {
                match message {
                    BenchmarkQueueMessage::Batch {
                        batch_id,
                        batch,
                        completion,
                    } => {
                        released
                            .recv_timeout(Duration::from_secs(1))
                            .expect("release worker");
                        let event_count = batch.batch().events.len();
                        accepted_events += u64::try_from(event_count).expect("event count");
                        publish_benchmark_batch_report(
                            &worker_shared,
                            WasmBenchmarkSinkBatchReport {
                                batch_id,
                                event_count,
                                result: Ok(BenchmarkSinkStatus { accepted_events }),
                                logs: Vec::new(),
                            },
                            completion,
                        );
                    }
                    BenchmarkQueueMessage::Flush(_) => panic!("unexpected flush"),
                    BenchmarkQueueMessage::Close => {
                        let mut report = queue_test_flush_report();
                        report
                            .result
                            .as_mut()
                            .expect("guest report")
                            .accepted_events = accepted_events;
                        publish_benchmark_queue_close_report(&worker_shared, report);
                        break;
                    }
                }
            }
        });
        let queue = WasmBenchmarkSinkQueue {
            inner: Arc::new(WasmBenchmarkSinkQueueInner {
                shared,
                worker: WasmWorker::new(worker),
            }),
        };
        let ticket = queue.submit(queue_test_batch(2)).expect("accepted batch");
        let batch_id = ticket.invocation_id();
        assert_eq!(
            ticket.wait(Duration::ZERO),
            Err(WasmPluginHostError::WaitExpired {
                operation: WasmWaitOperation::BatchResult,
                invocation_id: Some(batch_id),
            })
        );
        assert_eq!(queue.settlement().pending_invocations, 1);
        release.send(()).expect("release worker");
        let close = queue.close(Duration::from_secs(1)).expect("settled close");
        assert_eq!(
            close
                .result
                .as_ref()
                .expect("guest work completed")
                .accepted_events,
            2
        );
        assert_eq!(close.settlement.completed_invocations, 1);
        assert_eq!(close.settlement.failed_invocations, 0);
        assert_eq!(close.settlement.pending_invocations, 0);
        assert_eq!(close.settlement.wait_expirations, 1);
        assert!(close.settlement.worker_joined);
        assert!(close.settlement.quarantine_reason.is_none());
        let reports = queue.drain_reports();
        assert_eq!(reports.reports.len(), 1);
        assert_eq!(reports.reports[0].batch_id, batch_id);
        assert_eq!(
            reports.reports[0]
                .result
                .as_ref()
                .expect("late result")
                .accepted_events,
            2
        );
        assert_eq!(
            queue.close(Duration::ZERO).expect("idempotent close"),
            close
        );
    }

    #[test]
    fn benchmark_flush_keeps_the_completed_barrier_snapshot() {
        let shared = WasmBenchmarkSinkQueueShared::default();
        let reply = match begin_benchmark_queue_flush(&shared).expect("flush") {
            BenchmarkQueueFlushRequest::Pending(reply) => reply,
            _ => panic!("expected pending flush"),
        };
        complete_benchmark_queue_flush(&shared, &reply, queue_test_flush_report());
        enqueue_benchmark_batch(&shared, queue_test_checked_batch(1), None).expect("later batch");
        let report =
            wait_for_benchmark_queue_flush(&reply, Duration::ZERO).expect("flush snapshot");
        assert_eq!(report.settlement.accepted_invocations, 0);
        assert_eq!(
            shared
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .accepted_invocations,
            1
        );
    }

    #[test]
    fn benchmark_ids_never_wrap() {
        let shared = WasmBenchmarkSinkQueueShared::default();
        shared
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .next_batch_id = u64::MAX;
        assert_eq!(
            enqueue_benchmark_batch(&shared, queue_test_checked_batch(1), None).expect("last id"),
            WasmBenchmarkBatchEnqueueStatus::Enqueued { batch_id: u64::MAX }
        );
        assert_eq!(
            enqueue_benchmark_batch(&shared, queue_test_checked_batch(1), None),
            Err(WasmPluginHostError::Queue(
                crate::WasmQueueError::IdExhausted
            ))
        );
    }
}
