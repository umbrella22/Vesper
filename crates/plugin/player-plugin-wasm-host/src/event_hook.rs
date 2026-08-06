use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use player_plugin::{
    MAX_PIPELINE_EVENT_INPUT_BYTES, MAX_PLUGIN_ATTRIBUTE_KEY_BYTES,
    MAX_PLUGIN_ATTRIBUTE_VALUE_BYTES, MAX_PLUGIN_ATTRIBUTES, MAX_PLUGIN_DIAGNOSTICS,
    MAX_PLUGIN_ERROR_MESSAGE_BYTES, MAX_PLUGIN_MEASUREMENTS, PipelineEvent,
    PipelineEventHookOutcome, PluginDiagnostic, PluginDiagnosticSeverity, PluginMeasurement,
};
use wasmtime::Store;
use wasmtime::component::{HasSelf, Linker};

use crate::bindings::event_hook;
use crate::bindings::event_hook::vesper::plugin::protocol as wit;
use crate::host_state::{WasmHostState, WasmPluginLogRecord};
use crate::{
    MAX_WASM_PLUGIN_OUTPUT_BYTES, WASM_PLUGIN_EVENT_TIMEOUT_MILLIS, WasmPluginHostError,
    WasmPluginRuntime,
};

pub const WASM_PLUGIN_EVENT_QUEUE_CAPACITY: usize = 1_024;
pub const WASM_PLUGIN_EVENT_REPORT_QUEUE_CAPACITY: usize = 1_024;

pub struct WasmPipelineEventHookSession {
    runtime: WasmPluginRuntime,
    store: Store<WasmHostState>,
    bindings: event_hook::EventHookPlugin,
    quarantined: bool,
}

impl fmt::Debug for WasmPipelineEventHookSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WasmPipelineEventHookSession")
            .field("quarantined", &self.quarantined)
            .finish_non_exhaustive()
    }
}

impl WasmPipelineEventHookSession {
    pub fn from_component_bytes(
        runtime: &WasmPluginRuntime,
        bytes: &[u8],
    ) -> Result<Self, WasmPluginHostError> {
        let component = runtime.compile_component(bytes)?;
        let mut linker = Linker::new(runtime.engine());
        event_hook::EventHookPlugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)
            .map_err(|error| WasmPluginHostError::Instantiation(error.to_string()))?;
        let mut store =
            runtime.new_store(Duration::from_millis(WASM_PLUGIN_EVENT_TIMEOUT_MILLIS))?;
        let bindings = event_hook::EventHookPlugin::instantiate(&mut store, &component, &linker)
            .map_err(|error| WasmPluginHostError::Instantiation(error.to_string()))?;
        Ok(Self {
            runtime: runtime.clone(),
            store,
            bindings,
            quarantined: false,
        })
    }

    pub fn on_event(
        &mut self,
        event: &PipelineEvent,
    ) -> Result<PipelineEventHookOutcome, WasmPluginHostError> {
        self.ensure_available()?;
        validate_event_input(event)?;
        let input = pipeline_event_to_wit(event);
        self.runtime.prepare_store(
            &mut self.store,
            Duration::from_millis(WASM_PLUGIN_EVENT_TIMEOUT_MILLIS),
        )?;
        let result = self
            .bindings
            .vesper_plugin_event_hook()
            .call_on_event(&mut self.store, &input)
            .map_err(|error| self.quarantine_execution(error))?;
        let outcome = match result {
            Ok(outcome) => event_hook_outcome_from_wit(outcome)
                .map_err(|message| self.quarantine_protocol(message))?,
            Err(error) => return Err(self.map_plugin_error(error)),
        };
        if let Err(error) = outcome.validate() {
            return Err(self.quarantine_protocol(error.to_string()));
        }
        Ok(outcome)
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
        let (message, map): (String, fn(String) -> WasmPluginHostError) = match error {
            wit::PluginError::InvalidInput(message) => (message, WasmPluginHostError::InvalidInput),
            wit::PluginError::Rejected(message) => (message, WasmPluginHostError::Rejected),
            wit::PluginError::Failed(message) => (message, WasmPluginHostError::PluginFailed),
        };
        if message.is_empty() || message.len() > MAX_PLUGIN_ERROR_MESSAGE_BYTES {
            return self.quarantine_protocol(format!(
                "plugin error must contain 1 to {MAX_PLUGIN_ERROR_MESSAGE_BYTES} UTF-8 bytes"
            ));
        }
        map(message)
    }
}

fn validate_event_input(event: &PipelineEvent) -> Result<(), WasmPluginHostError> {
    event
        .validate()
        .map_err(|error| WasmPluginHostError::InvalidInput(error.to_string()))?;
    let mut size = 128_usize;
    for value in [
        Some(event.run_id.as_str()),
        Some(event.session_id.as_str()),
        Some(event.platform.as_str()),
        event.protocol.as_deref(),
        Some(event.event_name.as_str()),
        event.thread.as_deref(),
        event.resource_identity.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        add_payload_bytes(&mut size, value.len(), "pipeline event input size overflow")?;
    }
    add_attribute_bytes(
        &mut size,
        &event.attributes,
        "pipeline event input size overflow",
    )?;
    if let Some(diagnostic) = &event.diagnostic {
        add_payload_bytes(
            &mut size,
            diagnostic.code.len(),
            "pipeline event input size overflow",
        )?;
        add_payload_bytes(
            &mut size,
            diagnostic.message.len(),
            "pipeline event input size overflow",
        )?;
        add_attribute_bytes(
            &mut size,
            &diagnostic.attributes,
            "pipeline event input size overflow",
        )?;
    }
    if size > MAX_PIPELINE_EVENT_INPUT_BYTES {
        return Err(WasmPluginHostError::InvalidInput(format!(
            "pipeline event exceeds the {MAX_PIPELINE_EVENT_INPUT_BYTES}-byte transport limit"
        )));
    }
    Ok(())
}

fn pipeline_event_to_wit(event: &PipelineEvent) -> wit::PipelineEvent {
    wit::PipelineEvent {
        run_id: event.run_id.clone(),
        session_id: event.session_id.clone(),
        platform: event.platform.clone(),
        protocol: event.protocol.clone(),
        event_name: event.event_name.clone(),
        timestamp_ns: event.timestamp_ns,
        thread: event.thread.clone(),
        resource_identity: event.resource_identity.clone(),
        attributes: attributes_to_wit(&event.attributes),
        diagnostic: event.diagnostic.as_ref().map(diagnostic_to_wit),
    }
}

fn diagnostic_to_wit(diagnostic: &PluginDiagnostic) -> wit::Diagnostic {
    wit::Diagnostic {
        code: diagnostic.code.clone(),
        severity: match diagnostic.severity {
            PluginDiagnosticSeverity::Info => wit::DiagnosticSeverity::Info,
            PluginDiagnosticSeverity::Warning => wit::DiagnosticSeverity::Warning,
            PluginDiagnosticSeverity::Error => wit::DiagnosticSeverity::Error,
        },
        message: diagnostic.message.clone(),
        attributes: attributes_to_wit(&diagnostic.attributes),
    }
}

fn attributes_to_wit(attributes: &BTreeMap<String, String>) -> Vec<wit::Attribute> {
    attributes
        .iter()
        .map(|(key, value)| wit::Attribute {
            key: key.clone(),
            value: value.clone(),
        })
        .collect()
}

fn event_hook_outcome_from_wit(
    outcome: wit::EventHookOutcome,
) -> Result<PipelineEventHookOutcome, String> {
    if outcome.measurements.len() > MAX_PLUGIN_MEASUREMENTS {
        return Err(format!(
            "event-hook outcome exceeds the {MAX_PLUGIN_MEASUREMENTS}-measurement protocol limit"
        ));
    }
    if outcome.diagnostics.len() > MAX_PLUGIN_DIAGNOSTICS {
        return Err(format!(
            "event-hook outcome exceeds the {MAX_PLUGIN_DIAGNOSTICS}-diagnostic protocol limit"
        ));
    }
    validate_output_size(&outcome)?;
    Ok(PipelineEventHookOutcome {
        accepted: outcome.accepted,
        measurements: outcome
            .measurements
            .into_iter()
            .map(measurement_from_wit)
            .collect::<Result<_, _>>()?,
        diagnostics: outcome
            .diagnostics
            .into_iter()
            .map(diagnostic_from_wit)
            .collect::<Result<_, _>>()?,
    })
}

fn validate_output_size(outcome: &wit::EventHookOutcome) -> Result<(), String> {
    let mut size = 32_usize;
    size = size
        .checked_add(outcome.measurements.len().saturating_mul(32))
        .and_then(|size| size.checked_add(outcome.diagnostics.len().saturating_mul(32)))
        .ok_or_else(|| "event-hook output size overflow".to_owned())?;
    for measurement in &outcome.measurements {
        add_output_bytes(&mut size, measurement.name.len())?;
        add_output_bytes(&mut size, measurement.unit.len())?;
        add_wit_attribute_bytes(&mut size, &measurement.attributes)?;
    }
    for diagnostic in &outcome.diagnostics {
        add_output_bytes(&mut size, diagnostic.code.len())?;
        add_output_bytes(&mut size, diagnostic.message.len())?;
        add_wit_attribute_bytes(&mut size, &diagnostic.attributes)?;
    }
    if size > MAX_WASM_PLUGIN_OUTPUT_BYTES {
        return Err(format!(
            "event-hook output exceeds the {MAX_WASM_PLUGIN_OUTPUT_BYTES}-byte WASM output limit"
        ));
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

fn add_attribute_bytes(
    size: &mut usize,
    attributes: &BTreeMap<String, String>,
    overflow_message: &'static str,
) -> Result<(), WasmPluginHostError> {
    for (key, value) in attributes {
        add_payload_bytes(size, 16, overflow_message)?;
        add_payload_bytes(size, key.len(), overflow_message)?;
        add_payload_bytes(size, value.len(), overflow_message)?;
    }
    Ok(())
}

fn add_payload_bytes(
    size: &mut usize,
    added: usize,
    overflow_message: &'static str,
) -> Result<(), WasmPluginHostError> {
    *size = size
        .checked_add(added)
        .ok_or_else(|| WasmPluginHostError::InvalidInput(overflow_message.to_owned()))?;
    Ok(())
}

fn add_output_bytes(size: &mut usize, added: usize) -> Result<(), String> {
    *size = size
        .checked_add(added)
        .ok_or_else(|| "event-hook output size overflow".to_owned())?;
    Ok(())
}

fn add_wit_attribute_bytes(size: &mut usize, attributes: &[wit::Attribute]) -> Result<(), String> {
    add_output_bytes(size, attributes.len().saturating_mul(16))?;
    for attribute in attributes {
        add_output_bytes(size, attribute.key.len())?;
        add_output_bytes(size, attribute.value.len())?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmPipelineEventEnqueueStatus {
    Enqueued,
    Dropped,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WasmPipelineEventHookReport {
    pub run_id: String,
    pub session_id: String,
    pub event_name: String,
    pub result: Result<PipelineEventHookOutcome, WasmPluginHostError>,
    pub logs: Vec<WasmPluginLogRecord>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct WasmPipelineEventHookReportBatch {
    pub reports: Vec<WasmPipelineEventHookReport>,
    pub dropped_events: u64,
    pub dropped_reports: u64,
}

#[derive(Debug)]
enum QueueMessage {
    Event(PipelineEvent),
    Barrier(SyncSender<()>),
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventQueueLifecycle {
    Open,
    Closing,
    Closed,
}

#[derive(Debug)]
struct EventQueueState {
    lifecycle: EventQueueLifecycle,
    messages: VecDeque<QueueMessage>,
    queued_events: usize,
    flush_pending: bool,
    worker_error: Option<String>,
}

impl Default for EventQueueState {
    fn default() -> Self {
        Self {
            lifecycle: EventQueueLifecycle::Open,
            messages: VecDeque::new(),
            queued_events: 0,
            flush_pending: false,
            worker_error: None,
        }
    }
}

#[derive(Debug, Default)]
struct QueueReportSink {
    reports: Mutex<VecDeque<WasmPipelineEventHookReport>>,
    dropped_reports: AtomicU64,
}

impl QueueReportSink {
    fn push(&self, report: WasmPipelineEventHookReport) {
        let mut reports = self
            .reports
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if reports.len() >= WASM_PLUGIN_EVENT_REPORT_QUEUE_CAPACITY {
            self.dropped_reports.fetch_add(1, Ordering::Relaxed);
            return;
        }
        reports.push_back(report);
    }

    fn drain(&self) -> Vec<WasmPipelineEventHookReport> {
        self.reports
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .drain(..)
            .collect()
    }
}

#[derive(Debug)]
struct WasmPipelineEventHookQueueInner {
    shared: Arc<EventQueueShared>,
    worker: Mutex<EventWorkerJoinState>,
    worker_changed: Condvar,
}

impl Drop for WasmPipelineEventHookQueueInner {
    fn drop(&mut self) {
        request_event_queue_close(&self.shared);
    }
}

#[derive(Debug, Default)]
struct EventQueueShared {
    state: Mutex<EventQueueState>,
    message_available: Condvar,
    reports: QueueReportSink,
    dropped_events: AtomicU64,
}

#[derive(Debug)]
enum EventWorkerJoinState {
    Running(JoinHandle<()>),
    Joining,
    Joined(Result<(), String>),
}

#[derive(Debug, Clone)]
pub struct WasmPipelineEventHookQueue {
    inner: Arc<WasmPipelineEventHookQueueInner>,
}

impl WasmPipelineEventHookQueue {
    pub fn from_component_bytes(
        runtime: &WasmPluginRuntime,
        bytes: &[u8],
    ) -> Result<Self, WasmPluginHostError> {
        let session = WasmPipelineEventHookSession::from_component_bytes(runtime, bytes)?;
        let shared = Arc::new(EventQueueShared::default());
        let worker_shared = Arc::clone(&shared);
        let worker = std::thread::Builder::new()
            .name("vesper-wasm-event-hook".to_owned())
            .spawn(move || run_queue_worker(worker_shared, session))
            .map_err(|error| {
                WasmPluginHostError::Instantiation(format!(
                    "failed to start WASM event-hook worker: {error}"
                ))
            })?;
        Ok(Self {
            inner: Arc::new(WasmPipelineEventHookQueueInner {
                shared,
                worker: Mutex::new(EventWorkerJoinState::Running(worker)),
                worker_changed: Condvar::new(),
            }),
        })
    }

    pub fn enqueue(
        &self,
        event: PipelineEvent,
    ) -> Result<WasmPipelineEventEnqueueStatus, WasmPluginHostError> {
        validate_event_input(&event)?;
        enqueue_event(&self.inner.shared, event)
    }

    pub fn flush(&self, timeout: Duration) -> Result<(), WasmPluginHostError> {
        let barrier = begin_event_queue_flush(&self.inner.shared)?;
        match barrier.recv_timeout(timeout) {
            Ok(()) => Ok(()),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(WasmPluginHostError::QueueTimeout(
                "event hook flush".to_owned(),
            )),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(WasmPluginHostError::Queue(
                "WASM event-hook worker stopped before flush completed".to_owned(),
            )),
        }
    }

    pub fn close(&self, timeout: Duration) -> Result<(), WasmPluginHostError> {
        request_event_queue_close(&self.inner.shared);
        let started = Instant::now();
        let mut state = self
            .inner
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.lifecycle != EventQueueLifecycle::Closed {
            let Some(remaining) = timeout.checked_sub(started.elapsed()) else {
                return Err(WasmPluginHostError::QueueTimeout(
                    "event hook close".to_owned(),
                ));
            };
            let waited =
                self.inner
                    .shared
                    .message_available
                    .wait_timeout_while(state, remaining, |state| {
                        state.lifecycle != EventQueueLifecycle::Closed
                    });
            let (next_state, timeout_result) = match waited {
                Ok(result) => result,
                Err(poisoned) => poisoned.into_inner(),
            };
            state = next_state;
            if timeout_result.timed_out() && state.lifecycle != EventQueueLifecycle::Closed {
                return Err(WasmPluginHostError::QueueTimeout(
                    "event hook close".to_owned(),
                ));
            }
        }
        let worker_error = state.worker_error.clone();
        drop(state);
        self.join_worker(started, timeout)?;
        if let Some(error) = worker_error {
            return Err(WasmPluginHostError::Queue(error));
        }
        Ok(())
    }

    pub fn drain_reports(&self) -> WasmPipelineEventHookReportBatch {
        WasmPipelineEventHookReportBatch {
            reports: self.inner.shared.reports.drain(),
            dropped_events: self.inner.shared.dropped_events.swap(0, Ordering::Relaxed),
            dropped_reports: self
                .inner
                .shared
                .reports
                .dropped_reports
                .swap(0, Ordering::Relaxed),
        }
    }

    fn join_worker(&self, started: Instant, timeout: Duration) -> Result<(), WasmPluginHostError> {
        loop {
            let mut worker_state = self
                .inner
                .worker
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match &*worker_state {
                EventWorkerJoinState::Joined(result) => {
                    return result.clone().map_err(WasmPluginHostError::Queue);
                }
                EventWorkerJoinState::Joining => {
                    let Some(remaining) = timeout.checked_sub(started.elapsed()) else {
                        return Err(WasmPluginHostError::QueueTimeout(
                            "event hook worker join".to_owned(),
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
                        && !matches!(*worker_state, EventWorkerJoinState::Joined(_))
                    {
                        return Err(WasmPluginHostError::QueueTimeout(
                            "event hook worker join".to_owned(),
                        ));
                    }
                    drop(worker_state);
                }
                EventWorkerJoinState::Running(worker) if worker.is_finished() => {
                    let previous =
                        std::mem::replace(&mut *worker_state, EventWorkerJoinState::Joining);
                    let worker = match previous {
                        EventWorkerJoinState::Running(worker) => worker,
                        other => {
                            *worker_state = other;
                            return Err(WasmPluginHostError::Queue(
                                "WASM event-hook worker join state changed unexpectedly".to_owned(),
                            ));
                        }
                    };
                    drop(worker_state);
                    let result = worker
                        .join()
                        .map_err(|_| "WASM event-hook worker panicked while joining".to_owned());
                    let mut worker_state = self
                        .inner
                        .worker
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    *worker_state = EventWorkerJoinState::Joined(result.clone());
                    self.inner.worker_changed.notify_all();
                    return result.map_err(WasmPluginHostError::Queue);
                }
                EventWorkerJoinState::Running(_) => {
                    let Some(remaining) = timeout.checked_sub(started.elapsed()) else {
                        return Err(WasmPluginHostError::QueueTimeout(
                            "event hook worker join".to_owned(),
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

fn enqueue_event(
    shared: &EventQueueShared,
    event: PipelineEvent,
) -> Result<WasmPipelineEventEnqueueStatus, WasmPluginHostError> {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.lifecycle != EventQueueLifecycle::Open {
        return Err(WasmPluginHostError::Queue(
            "WASM event-hook queue is not open".to_owned(),
        ));
    }
    if state.queued_events >= WASM_PLUGIN_EVENT_QUEUE_CAPACITY {
        saturating_atomic_increment(&shared.dropped_events);
        return Ok(WasmPipelineEventEnqueueStatus::Dropped);
    }
    state.queued_events += 1;
    state.messages.push_back(QueueMessage::Event(event));
    shared.message_available.notify_one();
    Ok(WasmPipelineEventEnqueueStatus::Enqueued)
}

fn begin_event_queue_flush(
    shared: &EventQueueShared,
) -> Result<mpsc::Receiver<()>, WasmPluginHostError> {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.lifecycle != EventQueueLifecycle::Open {
        return Err(WasmPluginHostError::Queue(
            "WASM event-hook queue is not open".to_owned(),
        ));
    }
    if state.flush_pending {
        return Err(WasmPluginHostError::Queue(
            "WASM event-hook flush is already pending".to_owned(),
        ));
    }
    let (barrier_tx, barrier_rx) = mpsc::sync_channel(0);
    state.flush_pending = true;
    state.messages.push_back(QueueMessage::Barrier(barrier_tx));
    shared.message_available.notify_one();
    Ok(barrier_rx)
}

fn request_event_queue_close(shared: &EventQueueShared) {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.lifecycle == EventQueueLifecycle::Open {
        state.lifecycle = EventQueueLifecycle::Closing;
        state.messages.push_back(QueueMessage::Close);
        shared.message_available.notify_one();
    }
}

fn take_event_queue_message(shared: &EventQueueShared) -> Option<QueueMessage> {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    loop {
        if let Some(message) = state.messages.pop_front() {
            if matches!(message, QueueMessage::Event(_)) {
                state.queued_events = state.queued_events.saturating_sub(1);
            }
            return Some(message);
        }
        if state.lifecycle == EventQueueLifecycle::Closed {
            return None;
        }
        state = shared
            .message_available
            .wait(state)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
}

fn complete_event_queue_flush(shared: &EventQueueShared, acknowledge: SyncSender<()>) {
    {
        let mut state = shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.flush_pending = false;
        shared.message_available.notify_all();
    }
    let _ = acknowledge.send(());
}

fn run_queue_worker(shared: Arc<EventQueueShared>, mut session: WasmPipelineEventHookSession) {
    let worker_result = catch_unwind(AssertUnwindSafe(|| {
        while let Some(message) = take_event_queue_message(&shared) {
            match message {
                QueueMessage::Event(event) => {
                    let result = catch_unwind(AssertUnwindSafe(|| session.on_event(&event)))
                        .unwrap_or_else(|_| {
                            session.quarantined = true;
                            Err(WasmPluginHostError::Execution(
                                "WASM event-hook worker panicked".to_owned(),
                            ))
                        });
                    let logs = session.take_logs();
                    shared.reports.push(WasmPipelineEventHookReport {
                        run_id: event.run_id,
                        session_id: event.session_id,
                        event_name: event.event_name,
                        result,
                        logs,
                    });
                }
                QueueMessage::Barrier(acknowledge) => {
                    complete_event_queue_flush(&shared, acknowledge);
                }
                QueueMessage::Close => break,
            }
        }
    }));
    let worker_error = if worker_result.is_err() {
        fail_event_queue_after_worker_panic(&shared);
        Some("WASM event-hook queue worker panicked".to_owned())
    } else {
        None
    };
    drop(session);
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    state.lifecycle = EventQueueLifecycle::Closed;
    state.flush_pending = false;
    state.worker_error = worker_error;
    shared.message_available.notify_all();
}

fn fail_event_queue_after_worker_panic(shared: &EventQueueShared) {
    let pending = {
        let mut state = shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.lifecycle = EventQueueLifecycle::Closing;
        state.queued_events = 0;
        state.flush_pending = false;
        state.messages.drain(..).collect::<Vec<_>>()
    };
    for message in pending {
        match message {
            QueueMessage::Event(event) => {
                shared.reports.push(WasmPipelineEventHookReport {
                    run_id: event.run_id,
                    session_id: event.session_id,
                    event_name: event.event_name,
                    result: Err(WasmPluginHostError::Quarantined),
                    logs: Vec::new(),
                });
            }
            QueueMessage::Barrier(_) | QueueMessage::Close => {}
        }
    }
    shared.message_available.notify_all();
}

fn saturating_atomic_increment(counter: &AtomicU64) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        Some(value.saturating_add(1))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue_test_event(timestamp_ns: u64) -> PipelineEvent {
        PipelineEvent {
            run_id: "run".to_owned(),
            session_id: "session".to_owned(),
            platform: "test".to_owned(),
            protocol: None,
            event_name: "test.event".to_owned(),
            timestamp_ns,
            thread: None,
            resource_identity: None,
            attributes: BTreeMap::new(),
            diagnostic: None,
        }
    }

    #[test]
    fn component_without_event_hook_exports_is_rejected() {
        let runtime = WasmPluginRuntime::new().expect("WASM runtime");
        let bytes = wat::parse_str("(component)").expect("empty component");

        assert!(matches!(
            WasmPipelineEventHookSession::from_component_bytes(&runtime, &bytes),
            Err(WasmPluginHostError::Instantiation(message))
                if message.contains("event-hook")
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
    fn full_event_queue_drops_newest_without_replacing_the_queued_event() {
        let shared = EventQueueShared::default();
        for timestamp_ns in 0..WASM_PLUGIN_EVENT_QUEUE_CAPACITY {
            assert_eq!(
                enqueue_event(
                    &shared,
                    queue_test_event(u64::try_from(timestamp_ns).expect("test timestamp"))
                )
                .expect("accepted enqueue"),
                WasmPipelineEventEnqueueStatus::Enqueued
            );
        }
        assert_eq!(
            enqueue_event(&shared, queue_test_event(u64::MAX)).expect("dropped enqueue"),
            WasmPipelineEventEnqueueStatus::Dropped
        );

        let state = shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(state.queued_events, WASM_PLUGIN_EVENT_QUEUE_CAPACITY);
        assert_eq!(state.messages.len(), WASM_PLUGIN_EVENT_QUEUE_CAPACITY);
        assert!(
            matches!(state.messages.front(), Some(QueueMessage::Event(event)) if event.timestamp_ns == 0)
        );
        assert!(
            matches!(state.messages.back(), Some(QueueMessage::Event(event)) if event.timestamp_ns == 1_023)
        );
        assert_eq!(shared.dropped_events.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn full_event_queue_still_accepts_fifo_flush_and_close_controls() {
        let shared = EventQueueShared::default();
        for timestamp_ns in 0..WASM_PLUGIN_EVENT_QUEUE_CAPACITY {
            enqueue_event(
                &shared,
                queue_test_event(u64::try_from(timestamp_ns).expect("test timestamp")),
            )
            .expect("accepted enqueue");
        }

        let _flush = begin_event_queue_flush(&shared).expect("flush control");
        request_event_queue_close(&shared);

        let state = shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(state.lifecycle, EventQueueLifecycle::Closing);
        assert_eq!(state.queued_events, WASM_PLUGIN_EVENT_QUEUE_CAPACITY);
        assert_eq!(state.messages.len(), WASM_PLUGIN_EVENT_QUEUE_CAPACITY + 2);
        assert!(matches!(
            state.messages.get(WASM_PLUGIN_EVENT_QUEUE_CAPACITY),
            Some(QueueMessage::Barrier(_))
        ));
        assert!(matches!(
            state.messages.get(WASM_PLUGIN_EVENT_QUEUE_CAPACITY + 1),
            Some(QueueMessage::Close)
        ));
    }

    #[test]
    fn flush_timeout_does_not_poison_the_event_queue() {
        let shared = EventQueueShared::default();
        let first_barrier = begin_event_queue_flush(&shared).expect("first flush");

        assert_eq!(
            first_barrier.recv_timeout(Duration::from_millis(1)),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        drop(first_barrier);
        let message = take_event_queue_message(&shared).expect("queued barrier");
        let QueueMessage::Barrier(acknowledge) = message else {
            panic!("expected a barrier message");
        };
        complete_event_queue_flush(&shared, acknowledge);

        let second_barrier = begin_event_queue_flush(&shared).expect("second flush");
        let message = take_event_queue_message(&shared).expect("second queued barrier");
        let QueueMessage::Barrier(acknowledge) = message else {
            panic!("expected a barrier message");
        };
        drop(second_barrier);
        complete_event_queue_flush(&shared, acknowledge);
        let state = shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(!state.flush_pending);
    }

    #[test]
    fn close_timeout_keeps_closing_state_and_later_close_joins_worker() {
        let shared = Arc::new(EventQueueShared::default());
        let release_worker = Arc::new((Mutex::new(false), Condvar::new()));
        let worker_shared = Arc::clone(&shared);
        let worker_release = Arc::clone(&release_worker);
        let worker = std::thread::spawn(move || {
            assert!(matches!(
                take_event_queue_message(&worker_shared),
                Some(QueueMessage::Close)
            ));
            let (released, release_changed) = &*worker_release;
            let released = released
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let _released = release_changed
                .wait_while(released, |released| !*released)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut state = worker_shared
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.lifecycle = EventQueueLifecycle::Closed;
            worker_shared.message_available.notify_all();
        });
        let queue = WasmPipelineEventHookQueue {
            inner: Arc::new(WasmPipelineEventHookQueueInner {
                shared: Arc::clone(&shared),
                worker: Mutex::new(EventWorkerJoinState::Running(worker)),
                worker_changed: Condvar::new(),
            }),
        };

        request_event_queue_close(&shared);
        assert!(matches!(
            queue.close(Duration::from_millis(10)),
            Err(WasmPluginHostError::QueueTimeout(message))
                if message.contains("event hook close")
        ));
        assert_eq!(
            shared
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .lifecycle,
            EventQueueLifecycle::Closing
        );

        let (released, release_changed) = &*release_worker;
        *released
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
        release_changed.notify_all();

        queue
            .close(Duration::from_secs(1))
            .expect("close after release");
        queue.close(Duration::ZERO).expect("idempotent close");
    }
}
