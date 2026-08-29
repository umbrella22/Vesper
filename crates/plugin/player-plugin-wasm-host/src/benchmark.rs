use std::collections::{BTreeMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use player_plugin::{
    BenchmarkEvent, BenchmarkEventBatch, BenchmarkSinkReport, BenchmarkSinkStatus,
    BenchmarkThresholdViolation, MAX_BENCHMARK_THRESHOLD_VIOLATIONS,
    MAX_PLUGIN_ATTRIBUTE_KEY_BYTES, MAX_PLUGIN_ATTRIBUTE_VALUE_BYTES, MAX_PLUGIN_ATTRIBUTES,
    MAX_PLUGIN_DIAGNOSTICS, MAX_PLUGIN_ERROR_MESSAGE_BYTES, MAX_PLUGIN_MEASUREMENTS,
    PluginDiagnostic, PluginDiagnosticSeverity, PluginMeasurement,
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

pub struct WasmBenchmarkSinkSession {
    runtime: WasmPluginRuntime,
    store: Store<WasmHostState>,
    bindings: benchmark_sink::BenchmarkSinkPlugin,
    quarantined: bool,
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
        .map_err(|error| WasmPluginHostError::Instantiation(error.to_string()))?;
        let mut store =
            runtime.new_store(Duration::from_millis(WASM_PLUGIN_FLUSH_TIMEOUT_MILLIS))?;
        let bindings =
            benchmark_sink::BenchmarkSinkPlugin::instantiate(&mut store, &component, &linker)
                .map_err(|error| WasmPluginHostError::Instantiation(error.to_string()))?;
        Ok(Self {
            runtime: runtime.clone(),
            store,
            bindings,
            quarantined: false,
        })
    }

    pub fn on_event_batch(
        &mut self,
        batch: &BenchmarkEventBatch,
    ) -> Result<BenchmarkSinkStatus, WasmPluginHostError> {
        self.ensure_available()?;
        batch
            .validate()
            .map_err(|error| WasmPluginHostError::InvalidInput(error.to_string()))?;
        validate_batch_input_size(batch)?;
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
        if let Err(error) = report.validate() {
            return Err(self.quarantine_protocol(error.to_string()));
        }
        Ok(report)
    }

    pub fn take_logs(&mut self) -> Vec<WasmPluginLogRecord> {
        self.store.data_mut().take_logs()
    }

    pub fn is_quarantined(&self) -> bool {
        self.quarantined
    }

    fn ensure_available(&self) -> Result<(), WasmPluginHostError> {
        if self.quarantined {
            Err(WasmPluginHostError::Quarantined)
        } else {
            Ok(())
        }
    }

    fn quarantine_execution(&mut self, error: wasmtime::Error) -> WasmPluginHostError {
        self.quarantined = true;
        WasmPluginHostError::Execution(error.to_string())
    }

    fn quarantine_protocol(&mut self, message: String) -> WasmPluginHostError {
        self.quarantined = true;
        WasmPluginHostError::ProtocolViolation(message)
    }

    fn map_plugin_error(&mut self, error: wit::PluginError) -> WasmPluginHostError {
        match error {
            wit::PluginError::InvalidInput(message) => {
                bounded_plugin_error(message, WasmPluginHostError::InvalidInput, self)
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
    pub result: Result<BenchmarkSinkReport, WasmPluginHostError>,
    pub logs: Vec<WasmPluginLogRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BenchmarkQueueLifecycle {
    Open,
    Closing,
    Closed,
}

#[derive(Debug)]
enum BenchmarkQueueMessage {
    Batch {
        batch_id: u64,
        batch: BenchmarkEventBatch,
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
    lifecycle: BenchmarkQueueLifecycle,
    messages: VecDeque<BenchmarkQueueMessage>,
    queued_batches: usize,
    next_batch_id: u64,
    flush_pending: bool,
    completed_timed_out_flush: Option<WasmBenchmarkSinkFlushReport>,
    close_report: Option<WasmBenchmarkSinkFlushReport>,
}

impl Default for BenchmarkQueueState {
    fn default() -> Self {
        Self {
            lifecycle: BenchmarkQueueLifecycle::Open,
            messages: VecDeque::new(),
            queued_batches: 0,
            next_batch_id: 1,
            flush_pending: false,
            completed_timed_out_flush: None,
            close_report: None,
        }
    }
}

impl BenchmarkQueueState {
    fn allocate_batch_id(&mut self) -> Result<u64, WasmPluginHostError> {
        if self.next_batch_id == 0 {
            return Err(WasmPluginHostError::Queue(
                "WASM benchmark sink batch id space was exhausted".to_owned(),
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
    state: Mutex<BenchmarkQueueState>,
    message_available: Condvar,
    reports: BenchmarkQueueReportSink,
}

impl Default for WasmBenchmarkSinkQueueShared {
    fn default() -> Self {
        Self {
            state: Mutex::new(BenchmarkQueueState::default()),
            message_available: Condvar::new(),
            reports: BenchmarkQueueReportSink::default(),
        }
    }
}

#[derive(Debug)]
struct WasmBenchmarkSinkQueueInner {
    shared: Arc<WasmBenchmarkSinkQueueShared>,
    worker: Mutex<BenchmarkWorkerJoinState>,
    worker_changed: Condvar,
}

#[derive(Debug)]
enum BenchmarkWorkerJoinState {
    Running(JoinHandle<()>),
    Joining,
    Joined(Result<(), String>),
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
                WasmPluginHostError::Instantiation(format!(
                    "failed to start WASM benchmark-sink worker: {error}"
                ))
            })?;
        Ok(Self {
            inner: Arc::new(WasmBenchmarkSinkQueueInner {
                shared,
                worker: Mutex::new(BenchmarkWorkerJoinState::Running(worker)),
                worker_changed: Condvar::new(),
            }),
        })
    }

    pub fn enqueue(
        &self,
        batch: BenchmarkEventBatch,
    ) -> Result<WasmBenchmarkBatchEnqueueStatus, WasmPluginHostError> {
        validate_benchmark_batch(&batch)?;
        enqueue_benchmark_batch(&self.inner.shared, batch, None)
    }

    /// Executes one batch through the bounded worker queue and waits for its
    /// matching result for at most `timeout`.
    pub fn submit(
        &self,
        batch: BenchmarkEventBatch,
        timeout: Duration,
    ) -> Result<WasmBenchmarkSinkBatchReport, WasmPluginHostError> {
        validate_benchmark_batch(&batch)?;
        let (sender, receiver) = mpsc::sync_channel(1);
        match enqueue_benchmark_batch(&self.inner.shared, batch, Some(sender))? {
            WasmBenchmarkBatchEnqueueStatus::Enqueued { .. } => {}
            WasmBenchmarkBatchEnqueueStatus::Dropped { .. } => {
                return Err(WasmPluginHostError::Queue(
                    "WASM benchmark sink queue is full".to_owned(),
                ));
            }
        }
        match receiver.recv_timeout(timeout) {
            Ok(report) => Ok(report),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(WasmPluginHostError::QueueTimeout(
                "benchmark sink batch result".to_owned(),
            )),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(WasmPluginHostError::Queue(
                "WASM benchmark sink worker stopped before returning a batch result".to_owned(),
            )),
        }
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
                return Err(WasmPluginHostError::QueueTimeout(
                    "benchmark sink close".to_owned(),
                ));
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
                return Err(WasmPluginHostError::QueueTimeout(
                    "benchmark sink close".to_owned(),
                ));
            }
        }
        let report = state.close_report.clone().ok_or_else(|| {
            WasmPluginHostError::Queue(
                "WASM benchmark sink queue closed without a final report".to_owned(),
            )
        })?;
        drop(state);
        self.join_worker(started, timeout)?;
        let mut state = self
            .inner
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.lifecycle = BenchmarkQueueLifecycle::Closed;
        self.inner.shared.message_available.notify_all();
        Ok(report)
    }

    pub fn drain_reports(&self) -> WasmBenchmarkSinkQueueReportBatch {
        self.inner.shared.reports.drain()
    }

    fn join_worker(&self, started: Instant, timeout: Duration) -> Result<(), WasmPluginHostError> {
        loop {
            let mut worker_state = self
                .inner
                .worker
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match &*worker_state {
                BenchmarkWorkerJoinState::Joined(result) => {
                    return result.clone().map_err(WasmPluginHostError::Execution);
                }
                BenchmarkWorkerJoinState::Joining => {
                    let Some(remaining) = timeout.checked_sub(started.elapsed()) else {
                        return Err(WasmPluginHostError::QueueTimeout(
                            "benchmark sink worker join".to_owned(),
                        ));
                    };
                    let waited = self
                        .inner
                        .worker_changed
                        .wait_timeout(worker_state, remaining);
                    let (next_state, timeout_result) = match waited {
                        Ok(result) => result,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    worker_state = next_state;
                    if timeout_result.timed_out()
                        && !matches!(*worker_state, BenchmarkWorkerJoinState::Joined(_))
                    {
                        return Err(WasmPluginHostError::QueueTimeout(
                            "benchmark sink worker join".to_owned(),
                        ));
                    }
                    drop(worker_state);
                }
                BenchmarkWorkerJoinState::Running(worker) if worker.is_finished() => {
                    let previous =
                        std::mem::replace(&mut *worker_state, BenchmarkWorkerJoinState::Joining);
                    let worker = match previous {
                        BenchmarkWorkerJoinState::Running(worker) => worker,
                        other => {
                            *worker_state = other;
                            return Err(WasmPluginHostError::Queue(
                                "WASM benchmark sink worker join state changed unexpectedly"
                                    .to_owned(),
                            ));
                        }
                    };
                    drop(worker_state);
                    let result = worker.join().map_err(|_| {
                        "WASM benchmark sink worker panicked while joining".to_owned()
                    });
                    let mut worker_state = self
                        .inner
                        .worker
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    *worker_state = BenchmarkWorkerJoinState::Joined(result.clone());
                    self.inner.worker_changed.notify_all();
                    return result.map_err(WasmPluginHostError::Execution);
                }
                BenchmarkWorkerJoinState::Running(_) => {
                    let Some(remaining) = timeout.checked_sub(started.elapsed()) else {
                        return Err(WasmPluginHostError::QueueTimeout(
                            "benchmark sink worker join".to_owned(),
                        ));
                    };
                    let poll_interval = remaining.min(Duration::from_millis(1));
                    let waited = self
                        .inner
                        .worker_changed
                        .wait_timeout(worker_state, poll_interval);
                    worker_state = match waited {
                        Ok((state, _)) => state,
                        Err(poisoned) => poisoned.into_inner().0,
                    };
                    drop(worker_state);
                }
            }
        }
    }
}

fn validate_benchmark_batch(batch: &BenchmarkEventBatch) -> Result<(), WasmPluginHostError> {
    batch
        .validate()
        .map_err(|error| WasmPluginHostError::InvalidInput(error.to_string()))?;
    validate_batch_input_size(batch)
}

fn enqueue_benchmark_batch(
    shared: &WasmBenchmarkSinkQueueShared,
    batch: BenchmarkEventBatch,
    completion: Option<SyncSender<WasmBenchmarkSinkBatchReport>>,
) -> Result<WasmBenchmarkBatchEnqueueStatus, WasmPluginHostError> {
    let event_count = u64::try_from(batch.events.len()).unwrap_or(u64::MAX);
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.lifecycle != BenchmarkQueueLifecycle::Open {
        return Err(WasmPluginHostError::Queue(
            "WASM benchmark sink queue is not open".to_owned(),
        ));
    }
    let batch_id = state.allocate_batch_id()?;
    if state.queued_batches >= WASM_PLUGIN_BENCHMARK_BATCH_QUEUE_CAPACITY {
        shared.reports.record_queue_drop(event_count);
        return Ok(WasmBenchmarkBatchEnqueueStatus::Dropped { batch_id });
    }
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
    if state.lifecycle != BenchmarkQueueLifecycle::Open {
        return Err(WasmPluginHostError::Queue(
            "WASM benchmark sink queue is not open".to_owned(),
        ));
    }
    if let Some(report) = state.completed_timed_out_flush.take() {
        return Ok(BenchmarkQueueFlushRequest::Completed(report));
    }
    if state.flush_pending {
        return Err(WasmPluginHostError::Queue(
            "WASM benchmark sink flush is already pending".to_owned(),
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
    Err(WasmPluginHostError::QueueTimeout(
        "benchmark sink flush".to_owned(),
    ))
}

fn request_benchmark_queue_close(shared: &WasmBenchmarkSinkQueueShared) {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.lifecycle == BenchmarkQueueLifecycle::Open {
        state.lifecycle = BenchmarkQueueLifecycle::Closing;
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
        if state.lifecycle == BenchmarkQueueLifecycle::Closed {
            return None;
        }
        state = shared
            .message_available
            .wait(state)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
                    result: Err(WasmPluginHostError::Queue(
                        "WASM benchmark sink worker stopped without a close message".to_owned(),
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
                    let event_count = batch.events.len();
                    active_batch = Some((batch_id, event_count));
                    let result = call_benchmark_batch(&mut session, &batch);
                    let logs = session.take_logs();
                    let report = WasmBenchmarkSinkBatchReport {
                        batch_id,
                        event_count,
                        result,
                        logs,
                    };
                    if let Some(completion) = completion {
                        let _ = completion.send(report.clone());
                    }
                    shared.reports.push(report);
                    active_batch = None;
                }
                BenchmarkQueueMessage::Flush(reply) => {
                    let report = flush_benchmark_session(&mut session);
                    complete_benchmark_queue_flush(&shared, &reply, report);
                }
                BenchmarkQueueMessage::Close => break flush_benchmark_session(&mut session),
            }
        }
    }));
    let report = match worker_result {
        Ok(report) => report,
        Err(_) => {
            session.quarantined = true;
            fail_benchmark_queue_after_worker_panic(&shared, active_batch.take());
            WasmBenchmarkSinkFlushReport {
                result: Err(WasmPluginHostError::Execution(
                    "WASM benchmark sink worker panicked".to_owned(),
                )),
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
    report: WasmBenchmarkSinkFlushReport,
) {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut reply_state = reply
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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
    batch: &BenchmarkEventBatch,
) -> Result<BenchmarkSinkStatus, WasmPluginHostError> {
    catch_unwind(AssertUnwindSafe(|| session.on_event_batch(batch))).unwrap_or_else(|_| {
        session.quarantined = true;
        Err(WasmPluginHostError::Execution(
            "WASM benchmark sink batch worker panicked".to_owned(),
        ))
    })
}

fn flush_benchmark_session(session: &mut WasmBenchmarkSinkSession) -> WasmBenchmarkSinkFlushReport {
    let result = catch_unwind(AssertUnwindSafe(|| session.flush())).unwrap_or_else(|_| {
        session.quarantined = true;
        Err(WasmPluginHostError::Execution(
            "WASM benchmark sink flush worker panicked".to_owned(),
        ))
    });
    WasmBenchmarkSinkFlushReport {
        result,
        logs: session.take_logs(),
    }
}

fn fail_benchmark_queue_after_worker_panic(
    shared: &WasmBenchmarkSinkQueueShared,
    active_batch: Option<(u64, usize)>,
) {
    if let Some((batch_id, event_count)) = active_batch {
        shared.reports.push(WasmBenchmarkSinkBatchReport {
            batch_id,
            event_count,
            result: Err(WasmPluginHostError::Execution(
                "WASM benchmark sink worker panicked while processing the batch".to_owned(),
            )),
            logs: Vec::new(),
        });
    }
    let pending = {
        let mut state = shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.lifecycle = BenchmarkQueueLifecycle::Closing;
        state.queued_batches = 0;
        state.flush_pending = false;
        state.messages.drain(..).collect::<Vec<_>>()
    };
    for message in pending {
        match message {
            BenchmarkQueueMessage::Batch {
                batch_id,
                batch,
                completion,
            } => {
                let report = WasmBenchmarkSinkBatchReport {
                    batch_id,
                    event_count: batch.events.len(),
                    result: Err(WasmPluginHostError::Quarantined),
                    logs: Vec::new(),
                };
                if let Some(completion) = completion {
                    let _ = completion.send(report.clone());
                }
                shared.reports.push(report);
            }
            BenchmarkQueueMessage::Flush(reply) => {
                complete_benchmark_queue_flush(
                    shared,
                    &reply,
                    WasmBenchmarkSinkFlushReport {
                        result: Err(WasmPluginHostError::Execution(
                            "WASM benchmark sink worker panicked before flush".to_owned(),
                        )),
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
    report: WasmBenchmarkSinkFlushReport,
) {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.close_report.is_none() {
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
                WasmPluginHostError::InvalidInput("benchmark input size overflow".to_owned())
            })?;
        }
        for (key, value) in &event.attributes {
            size = size
                .checked_add(key.len())
                .and_then(|size| size.checked_add(value.len()))
                .ok_or_else(|| {
                    WasmPluginHostError::InvalidInput("benchmark input size overflow".to_owned())
                })?;
        }
        if size > MAX_WASM_PLUGIN_INPUT_BYTES {
            return Err(WasmPluginHostError::InvalidInput(format!(
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
    Ok(BenchmarkSinkReport {
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
    })
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
    if attributes.len() > MAX_PLUGIN_ATTRIBUTES {
        return Err(format!(
            "plugin attributes exceed the {MAX_PLUGIN_ATTRIBUTES}-entry protocol limit"
        ));
    }
    let mut result = BTreeMap::new();
    for attribute in attributes {
        if attribute.key.is_empty() || attribute.key.len() > MAX_PLUGIN_ATTRIBUTE_KEY_BYTES {
            return Err(format!(
                "attribute.key must contain 1 to {MAX_PLUGIN_ATTRIBUTE_KEY_BYTES} UTF-8 bytes"
            ));
        }
        if attribute.value.is_empty() || attribute.value.len() > MAX_PLUGIN_ATTRIBUTE_VALUE_BYTES {
            return Err(format!(
                "attribute.value must contain 1 to {MAX_PLUGIN_ATTRIBUTE_VALUE_BYTES} UTF-8 bytes"
            ));
        }
        if result
            .insert(attribute.key.clone(), attribute.value)
            .is_some()
        {
            return Err(format!(
                "plugin returned duplicate attribute key '{}'",
                attribute.key
            ));
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Barrier, mpsc};

    use super::*;

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

    fn queue_test_flush_report() -> WasmBenchmarkSinkFlushReport {
        WasmBenchmarkSinkFlushReport {
            result: Ok(BenchmarkSinkReport::default()),
            logs: Vec::new(),
        }
    }

    #[test]
    fn component_without_benchmark_exports_is_rejected() {
        let runtime = WasmPluginRuntime::new().expect("WASM runtime");
        let bytes = wat::parse_str("(component)").expect("empty component");

        assert!(matches!(
            WasmBenchmarkSinkSession::from_component_bytes(&runtime, &bytes),
            Err(WasmPluginHostError::Instantiation(message))
                if message.contains("benchmark-sink")
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
                enqueue_benchmark_batch(&shared, queue_test_batch(2), None)
                    .expect("accepted batch"),
                WasmBenchmarkBatchEnqueueStatus::Enqueued {
                    batch_id: u64::try_from(expected_id).expect("batch id"),
                }
            );
        }
        assert_eq!(
            enqueue_benchmark_batch(&shared, queue_test_batch(3), None).expect("dropped batch"),
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
            enqueue_benchmark_batch(&shared, queue_test_batch(1), None).expect("accepted batch");
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
        assert_eq!(state.lifecycle, BenchmarkQueueLifecycle::Closing);
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
                worker: Mutex::new(BenchmarkWorkerJoinState::Running(worker)),
                worker_changed: Condvar::new(),
            }),
        };

        assert_eq!(
            queue.flush(Duration::from_millis(10)),
            Err(WasmPluginHostError::QueueTimeout(
                "benchmark sink flush".to_owned()
            ))
        );
        assert_eq!(
            shared
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .lifecycle,
            BenchmarkQueueLifecycle::Open
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
            Ok(queue_test_flush_report())
        );
    }

    #[test]
    fn worker_panic_reports_the_active_batch_and_quarantines_pending_batches() {
        let shared = WasmBenchmarkSinkQueueShared::default();
        enqueue_benchmark_batch(&shared, queue_test_batch(1), None).expect("first batch");
        enqueue_benchmark_batch(&shared, queue_test_batch(2), None).expect("second batch");
        let active = match take_benchmark_queue_message(&shared).expect("active batch") {
            BenchmarkQueueMessage::Batch {
                batch_id, batch, ..
            } => (batch_id, batch.events.len()),
            BenchmarkQueueMessage::Flush(_) | BenchmarkQueueMessage::Close => {
                panic!("expected a batch message")
            }
        };

        fail_benchmark_queue_after_worker_panic(&shared, Some(active));

        let reports = shared.reports.drain().reports;
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].batch_id, 1);
        assert!(matches!(
            reports[0].result,
            Err(WasmPluginHostError::Execution(_))
        ));
        assert_eq!(reports[1].batch_id, 2);
        assert!(matches!(
            reports[1].result,
            Err(WasmPluginHostError::Quarantined)
        ));
        let state = shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(state.lifecycle, BenchmarkQueueLifecycle::Closing);
        assert_eq!(state.queued_batches, 0);
        assert!(state.messages.is_empty());
    }

    #[test]
    fn close_timeout_keeps_closing_state_and_later_close_is_idempotent() {
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
                worker: Mutex::new(BenchmarkWorkerJoinState::Running(worker)),
                worker_changed: Condvar::new(),
            }),
        };

        request_benchmark_queue_close(&shared);
        report_published.wait();
        assert!(matches!(
            queue.close(Duration::from_millis(10)),
            Err(WasmPluginHostError::QueueTimeout(message))
                if message.contains("worker join")
        ));
        assert_eq!(
            shared
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .lifecycle,
            BenchmarkQueueLifecycle::Closing
        );

        let (released, release_changed) = &*release_worker;
        *released
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
        release_changed.notify_all();

        let expected = queue_test_flush_report();
        assert_eq!(queue.close(Duration::from_secs(1)), Ok(expected.clone()));
        assert_eq!(queue.close(Duration::ZERO), Ok(expected));
        assert_eq!(
            shared
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .lifecycle,
            BenchmarkQueueLifecycle::Closed
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
                worker: Mutex::new(BenchmarkWorkerJoinState::Running(worker)),
                worker_changed: Condvar::new(),
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

        let expected = Ok(queue_test_flush_report());
        assert_eq!(
            result_rx.recv_timeout(Duration::from_secs(1)),
            Ok(expected.clone())
        );
        assert_eq!(result_rx.recv_timeout(Duration::from_secs(1)), Ok(expected));
        for caller in callers {
            caller.join().expect("close caller");
        }
    }
}
