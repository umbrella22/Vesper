use crate::invocation::WasmWorker;
use crate::{WasmInvocationTicket, WasmQueueLifecycle, WasmQueueSettlement, WasmWaitOperation};
use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use player_plugin::{
    MAX_PIPELINE_EVENT_INPUT_BYTES, MAX_PLUGIN_DIAGNOSTICS, MAX_PLUGIN_ERROR_MESSAGE_BYTES,
    MAX_PLUGIN_MEASUREMENTS, PipelineEvent, PipelineEventHookOutcome, PluginDiagnostic,
    PluginDiagnosticSeverity, PluginMeasurement,
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

/// Immutable owned input whose semantic and transport checks have succeeded.
#[derive(Debug)]
struct CheckedPipelineEvent(PipelineEvent);

impl CheckedPipelineEvent {
    fn new(event: PipelineEvent) -> Result<Self, WasmPluginHostError> {
        validate_event_input(&event)?;
        Ok(Self(event))
    }

    fn copy_from(event: &PipelineEvent) -> Result<Self, WasmPluginHostError> {
        // Bound borrowed data before allocating an owned queue copy.
        validate_event_input(event)?;
        Ok(Self(event.clone()))
    }

    fn event(&self) -> &PipelineEvent {
        &self.0
    }
}

pub struct WasmPipelineEventHookSession {
    runtime: WasmPluginRuntime,
    store: Store<WasmHostState>,
    bindings: event_hook::EventHookPlugin,
    quarantined: Option<WasmPluginHostError>,
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
            .map_err(|error| WasmPluginHostError::Instantiation(error.into()))?;
        let mut store =
            runtime.new_store(Duration::from_millis(WASM_PLUGIN_EVENT_TIMEOUT_MILLIS))?;
        let bindings = event_hook::EventHookPlugin::instantiate(&mut store, &component, &linker)
            .map_err(|error| WasmPluginHostError::Instantiation(error.into()))?;
        Ok(Self {
            runtime: runtime.clone(),
            store,
            bindings,
            quarantined: None,
        })
    }

    pub fn on_event(
        &mut self,
        event: &PipelineEvent,
    ) -> Result<PipelineEventHookOutcome, WasmPluginHostError> {
        self.ensure_available()?;
        validate_event_input(event)?;
        self.execute_event(event)
    }

    fn on_checked_event(
        &mut self,
        event: &CheckedPipelineEvent,
    ) -> Result<PipelineEventHookOutcome, WasmPluginHostError> {
        self.ensure_available()?;
        self.execute_event(event.event())
    }

    /// Both callers have established the input invariant at their boundary.
    fn execute_event(
        &mut self,
        event: &PipelineEvent,
    ) -> Result<PipelineEventHookOutcome, WasmPluginHostError> {
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
        Ok(outcome)
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
        let (message, map): (String, fn(String) -> WasmPluginHostError) = match error {
            wit::PluginError::InvalidInput(message) => {
                (message, WasmPluginHostError::GuestInvalidInput)
            }
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
        .map_err(|error| WasmPluginHostError::LocalValidation(error.to_string()))?;
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
        return Err(WasmPluginHostError::LocalValidation(format!(
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
    let outcome = PipelineEventHookOutcome {
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
    };
    outcome.validate().map_err(|error| error.to_string())?;
    Ok(outcome)
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
    crate::attributes::decode_attributes(
        attributes
            .into_iter()
            .map(|attribute| (attribute.key, attribute.value)),
    )
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
        .ok_or_else(|| WasmPluginHostError::LocalValidation(overflow_message.to_owned()))?;
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
    Enqueued { invocation_id: u64 },
    Dropped,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WasmPipelineEventHookReport {
    pub invocation_id: u64,
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
    Event {
        invocation_id: u64,
        event: Box<CheckedPipelineEvent>,
        completion: Option<SyncSender<WasmPipelineEventHookReport>>,
    },
    Barrier(SyncSender<WasmQueueSettlement>),
    Close,
}

#[derive(Debug)]
struct EventQueueState {
    lifecycle: WasmQueueLifecycle,
    messages: VecDeque<QueueMessage>,
    queued_events: usize,
    accepted_invocations: u64,
    completed_invocations: u64,
    failed_invocations: u64,
    quarantine_reason: Option<WasmPluginHostError>,
    next_event_id: u64,
    flush_pending: bool,
    worker_error: Option<WasmPluginHostError>,
}

impl Default for EventQueueState {
    fn default() -> Self {
        Self {
            lifecycle: WasmQueueLifecycle::Open,
            messages: VecDeque::new(),
            queued_events: 0,
            next_event_id: 1,
            accepted_invocations: 0,
            completed_invocations: 0,
            failed_invocations: 0,
            quarantine_reason: None,
            flush_pending: false,
            worker_error: None,
        }
    }
}

impl EventQueueState {
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
            saturating_atomic_increment(&self.dropped_reports);
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
    worker: WasmWorker,
}

impl Drop for WasmPipelineEventHookQueueInner {
    fn drop(&mut self) {
        request_event_queue_close(&self.shared);
    }
}

#[derive(Debug, Default)]
struct EventQueueShared {
    wait_expirations: Arc<AtomicU64>,
    state: Mutex<EventQueueState>,
    message_available: Condvar,
    reports: QueueReportSink,
    dropped_events: AtomicU64,
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
                WasmPluginHostError::Instantiation(wasmtime::Error::from(error).into())
            })?;
        Ok(Self {
            inner: Arc::new(WasmPipelineEventHookQueueInner {
                shared,
                worker: WasmWorker::new(worker),
            }),
        })
    }

    pub fn enqueue(
        &self,
        event: PipelineEvent,
    ) -> Result<WasmPipelineEventEnqueueStatus, WasmPluginHostError> {
        enqueue_event(&self.inner.shared, CheckedPipelineEvent::new(event)?, None)
    }

    /// Accepts one event without waiting for guest execution.
    pub fn submit(
        &self,
        event: PipelineEvent,
    ) -> Result<WasmInvocationTicket<WasmPipelineEventHookReport>, WasmPluginHostError> {
        self.submit_checked(CheckedPipelineEvent::new(event)?)
    }

    pub(crate) fn submit_borrowed(
        &self,
        event: &PipelineEvent,
    ) -> Result<WasmInvocationTicket<WasmPipelineEventHookReport>, WasmPluginHostError> {
        self.submit_checked(CheckedPipelineEvent::copy_from(event)?)
    }

    fn submit_checked(
        &self,
        event: CheckedPipelineEvent,
    ) -> Result<WasmInvocationTicket<WasmPipelineEventHookReport>, WasmPluginHostError> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let invocation_id = match enqueue_event(&self.inner.shared, event, Some(sender))? {
            WasmPipelineEventEnqueueStatus::Enqueued { invocation_id } => invocation_id,
            WasmPipelineEventEnqueueStatus::Dropped => {
                return Err(WasmPluginHostError::Queue(crate::WasmQueueError::Full));
            }
        };
        Ok(WasmInvocationTicket::new(
            invocation_id,
            receiver,
            WasmWaitOperation::EventResult,
            Arc::clone(&self.inner.shared.wait_expirations),
        ))
    }

    /// Accepts work, then waits. Expiry does not cancel accepted work.
    pub fn submit_and_wait(
        &self,
        event: PipelineEvent,
        timeout: Duration,
    ) -> Result<WasmPipelineEventHookReport, WasmPluginHostError> {
        self.submit(event)?.wait(timeout)
    }

    pub fn flush(&self, timeout: Duration) -> Result<WasmQueueSettlement, WasmPluginHostError> {
        let barrier = begin_event_queue_flush(&self.inner.shared)?;
        match barrier.recv_timeout(timeout) {
            Ok(settlement) => Ok(settlement),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(WasmPluginHostError::WaitExpired {
                operation: crate::WasmWaitOperation::Flush,
                invocation_id: None,
            }),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(WasmPluginHostError::Queue(
                crate::WasmQueueError::WorkerStopped,
            )),
        }
    }

    pub fn close(&self, timeout: Duration) -> Result<WasmQueueSettlement, WasmPluginHostError> {
        request_event_queue_close(&self.inner.shared);
        let started = Instant::now();
        let mut state = self
            .inner
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.lifecycle != WasmQueueLifecycle::Closed {
            let Some(remaining) = timeout.checked_sub(started.elapsed()) else {
                return Err(WasmPluginHostError::WaitExpired {
                    operation: crate::WasmWaitOperation::Close,
                    invocation_id: None,
                });
            };
            let waited =
                self.inner
                    .shared
                    .message_available
                    .wait_timeout_while(state, remaining, |state| {
                        state.lifecycle != WasmQueueLifecycle::Closed
                    });
            let (next_state, timeout_result) = match waited {
                Ok(result) => result,
                Err(poisoned) => poisoned.into_inner(),
            };
            state = next_state;
            if timeout_result.timed_out() && state.lifecycle != WasmQueueLifecycle::Closed {
                return Err(WasmPluginHostError::WaitExpired {
                    operation: crate::WasmWaitOperation::Close,
                    invocation_id: None,
                });
            }
        }
        let worker_error = state.worker_error.clone();
        drop(state);
        self.join_worker(started, timeout)?;
        if let Some(error) = worker_error {
            return Err(error);
        }
        Ok(self.settlement())
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

fn enqueue_event(
    shared: &EventQueueShared,
    event: CheckedPipelineEvent,
    completion: Option<SyncSender<WasmPipelineEventHookReport>>,
) -> Result<WasmPipelineEventEnqueueStatus, WasmPluginHostError> {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.lifecycle != WasmQueueLifecycle::Open {
        return Err(WasmPluginHostError::Queue(crate::WasmQueueError::Closed));
    }
    if state.queued_events >= WASM_PLUGIN_EVENT_QUEUE_CAPACITY {
        saturating_atomic_increment(&shared.dropped_events);
        return Ok(WasmPipelineEventEnqueueStatus::Dropped);
    }
    if state.next_event_id == 0 {
        return Err(WasmPluginHostError::Queue(
            crate::WasmQueueError::IdExhausted,
        ));
    }
    let invocation_id = state.next_event_id;
    state.next_event_id = invocation_id.checked_add(1).unwrap_or(0);
    state.accepted_invocations = state.accepted_invocations.saturating_add(1);
    state.queued_events += 1;
    state.messages.push_back(QueueMessage::Event {
        invocation_id,
        event: Box::new(event),
        completion,
    });
    shared.message_available.notify_one();
    Ok(WasmPipelineEventEnqueueStatus::Enqueued { invocation_id })
}

fn begin_event_queue_flush(
    shared: &EventQueueShared,
) -> Result<mpsc::Receiver<WasmQueueSettlement>, WasmPluginHostError> {
    let mut state = shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.lifecycle != WasmQueueLifecycle::Open {
        return Err(WasmPluginHostError::Queue(crate::WasmQueueError::Closed));
    }
    if state.flush_pending {
        return Err(WasmPluginHostError::Queue(
            crate::WasmQueueError::FlushPending,
        ));
    }
    let (barrier_tx, barrier_rx) = mpsc::sync_channel(1);
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
    if state.lifecycle == WasmQueueLifecycle::Open {
        state.lifecycle = WasmQueueLifecycle::Closing;
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
            if matches!(message, QueueMessage::Event { .. }) {
                state.queued_events = state.queued_events.saturating_sub(1);
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

fn complete_event_queue_flush(
    shared: &EventQueueShared,
    acknowledge: SyncSender<WasmQueueSettlement>,
) {
    let settlement = {
        let mut state = shared.state.lock().unwrap_or_else(|p| p.into_inner());
        state.flush_pending = false;
        shared.message_available.notify_all();
        state.settlement(shared.wait_expirations.load(Ordering::Relaxed), false)
    };
    // A discarded/expired waiter must never block the worker.
    let _ = acknowledge.try_send(settlement);
}

struct ActiveEvent {
    invocation_id: u64,
    run_id: String,
    session_id: String,
    event_name: String,
    completion: Option<SyncSender<WasmPipelineEventHookReport>>,
}

fn publish_event_report(
    shared: &EventQueueShared,
    report: WasmPipelineEventHookReport,
    completion: Option<SyncSender<WasmPipelineEventHookReport>>,
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

fn run_queue_worker(shared: Arc<EventQueueShared>, mut session: WasmPipelineEventHookSession) {
    let mut active_event = None;
    let worker_result = catch_unwind(AssertUnwindSafe(|| {
        while let Some(message) = take_event_queue_message(&shared) {
            match message {
                QueueMessage::Event {
                    invocation_id,
                    event,
                    completion,
                } => {
                    active_event = Some(ActiveEvent {
                        invocation_id,
                        run_id: event.event().run_id.clone(),
                        session_id: event.event().session_id.clone(),
                        event_name: event.event().event_name.clone(),
                        completion,
                    });
                    let result =
                        catch_unwind(AssertUnwindSafe(|| session.on_checked_event(&event)))
                            .unwrap_or_else(|_| {
                                let failure = WasmPluginHostError::WorkerPanic(
                                    "event-hook invocation".to_owned(),
                                );
                                session.quarantined = Some(failure.clone());
                                Err(failure)
                            });
                    {
                        let mut state = shared.state.lock().unwrap_or_else(|p| p.into_inner());
                        state.quarantine_reason = session.quarantined.clone();
                    }
                    let logs = session.take_logs();
                    let event = event.0;
                    let report = WasmPipelineEventHookReport {
                        invocation_id,
                        run_id: event.run_id,
                        session_id: event.session_id,
                        event_name: event.event_name,
                        result,
                        logs,
                    };
                    let completion = active_event.take().and_then(|active| active.completion);
                    publish_event_report(&shared, report, completion);
                }
                QueueMessage::Barrier(acknowledge) => {
                    complete_event_queue_flush(&shared, acknowledge);
                }
                QueueMessage::Close => break,
            }
        }
    }));
    let worker_error = if worker_result.is_err() {
        let reason = WasmPluginHostError::WorkerPanic("event-hook queue worker".to_owned());
        fail_event_queue_after_worker_panic(&shared, active_event.take(), &reason);
        Some(reason)
    } else {
        None
    };
    drop(session);
    let mut state = shared.state.lock().unwrap_or_else(|p| p.into_inner());
    state.lifecycle = WasmQueueLifecycle::Closed;
    state.flush_pending = false;
    state.worker_error = worker_error;
    shared.message_available.notify_all();
}

fn fail_event_queue_after_worker_panic(
    shared: &EventQueueShared,
    active_event: Option<ActiveEvent>,
    reason: &WasmPluginHostError,
) {
    let pending = {
        let mut state = shared.state.lock().unwrap_or_else(|p| p.into_inner());
        state.lifecycle = WasmQueueLifecycle::Closing;
        state.quarantine_reason = Some(reason.clone());
        state.queued_events = 0;
        state.flush_pending = false;
        state.messages.drain(..).collect::<Vec<_>>()
    };
    if let Some(ActiveEvent {
        invocation_id,
        run_id,
        session_id,
        event_name,
        completion,
    }) = active_event
    {
        publish_event_report(
            shared,
            WasmPipelineEventHookReport {
                invocation_id,
                run_id,
                session_id,
                event_name,
                result: Err(reason.clone()),
                logs: Vec::new(),
            },
            completion,
        );
    }
    for message in pending {
        match message {
            QueueMessage::Event {
                invocation_id,
                event,
                completion,
            } => {
                let event = event.0;
                let report = WasmPipelineEventHookReport {
                    invocation_id,
                    run_id: event.run_id,
                    session_id: event.session_id,
                    event_name: event.event_name,
                    result: Err(WasmPluginHostError::Quarantined {
                        reason: Arc::new(reason.clone()),
                    }),
                    logs: Vec::new(),
                };
                publish_event_report(shared, report, completion);
            }
            QueueMessage::Barrier(_) | QueueMessage::Close => {}
        }
    }
    shared.message_available.notify_all();
}

fn saturating_atomic_increment(counter: &AtomicU64) {
    let _ = counter.try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        Some(value.saturating_add(1))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue_test_raw_event(timestamp_ns: u64) -> PipelineEvent {
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

    fn queue_test_event(timestamp_ns: u64) -> CheckedPipelineEvent {
        CheckedPipelineEvent::new(queue_test_raw_event(timestamp_ns)).expect("checked event")
    }

    #[test]
    fn component_without_event_hook_exports_is_rejected() {
        let runtime = WasmPluginRuntime::new().expect("WASM runtime");
        let bytes = wat::parse_str("(component)").expect("empty component");

        assert!(matches!(
            WasmPipelineEventHookSession::from_component_bytes(&runtime, &bytes),
            Err(WasmPluginHostError::Instantiation(message))
                if message.to_string().contains("event-hook")
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
                    queue_test_event(u64::try_from(timestamp_ns).expect("test timestamp")),
                    None,
                )
                .expect("accepted enqueue"),
                WasmPipelineEventEnqueueStatus::Enqueued {
                    invocation_id: u64::try_from(timestamp_ns).expect("test id") + 1
                }
            );
        }
        assert_eq!(
            enqueue_event(&shared, queue_test_event(u64::MAX), None).expect("dropped enqueue"),
            WasmPipelineEventEnqueueStatus::Dropped
        );

        let state = shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(state.queued_events, WASM_PLUGIN_EVENT_QUEUE_CAPACITY);
        assert_eq!(state.messages.len(), WASM_PLUGIN_EVENT_QUEUE_CAPACITY);
        assert!(
            matches!(state.messages.front(), Some(QueueMessage::Event { event, .. }) if event.event().timestamp_ns == 0)
        );
        assert!(
            matches!(state.messages.back(), Some(QueueMessage::Event { event, .. }) if event.event().timestamp_ns == 1_023)
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
                None,
            )
            .expect("accepted enqueue");
        }

        let _flush = begin_event_queue_flush(&shared).expect("flush control");
        request_event_queue_close(&shared);

        let state = shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(state.lifecycle, WasmQueueLifecycle::Closing);
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
            state.lifecycle = WasmQueueLifecycle::Closed;
            worker_shared.message_available.notify_all();
        });
        let queue = WasmPipelineEventHookQueue {
            inner: Arc::new(WasmPipelineEventHookQueueInner {
                shared: Arc::clone(&shared),
                worker: WasmWorker::new(worker),
            }),
        };

        request_event_queue_close(&shared);
        assert!(matches!(
            queue.close(Duration::from_millis(10)),
            Err(WasmPluginHostError::WaitExpired {
                operation: crate::WasmWaitOperation::Close,
                ..
            })
        ));
        assert_eq!(
            shared
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .lifecycle,
            WasmQueueLifecycle::Closing
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

    #[test]
    fn expired_result_wait_keeps_accepted_work_and_correlated_late_report() {
        let shared = Arc::new(EventQueueShared::default());
        let worker_shared = Arc::clone(&shared);
        let (release, released) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            while let Some(message) = take_event_queue_message(&worker_shared) {
                match message {
                    QueueMessage::Event {
                        invocation_id,
                        event,
                        completion,
                    } => {
                        released
                            .recv_timeout(Duration::from_secs(1))
                            .expect("release worker");
                        let event = event.0;
                        publish_event_report(
                            &worker_shared,
                            WasmPipelineEventHookReport {
                                invocation_id,
                                run_id: event.run_id,
                                session_id: event.session_id,
                                event_name: event.event_name,
                                result: Ok(PipelineEventHookOutcome {
                                    accepted: true,
                                    ..Default::default()
                                }),
                                logs: Vec::new(),
                            },
                            completion,
                        );
                    }
                    QueueMessage::Barrier(reply) => {
                        complete_event_queue_flush(&worker_shared, reply)
                    }
                    QueueMessage::Close => break,
                }
            }
            let mut state = worker_shared
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            state.lifecycle = WasmQueueLifecycle::Closed;
            worker_shared.message_available.notify_all();
        });
        let queue = WasmPipelineEventHookQueue {
            inner: Arc::new(WasmPipelineEventHookQueueInner {
                shared,
                worker: WasmWorker::new(worker),
            }),
        };
        let ticket = queue
            .submit(queue_test_raw_event(1))
            .expect("accepted event");
        let invocation_id = ticket.invocation_id();
        assert_eq!(
            ticket.wait(Duration::ZERO),
            Err(WasmPluginHostError::WaitExpired {
                operation: WasmWaitOperation::EventResult,
                invocation_id: Some(invocation_id),
            })
        );
        let pending = queue.settlement();
        assert_eq!(pending.accepted_invocations, 1);
        assert_eq!(pending.completed_invocations, 0);
        assert_eq!(pending.pending_invocations, 1);
        assert_eq!(pending.wait_expirations, 1);
        release.send(()).expect("release worker");
        let settled = queue.close(Duration::from_secs(1)).expect("settled close");
        assert_eq!(settled.lifecycle, WasmQueueLifecycle::Closed);
        assert!(settled.worker_joined);
        assert_eq!(settled.completed_invocations, 1);
        assert_eq!(settled.failed_invocations, 0);
        assert_eq!(settled.pending_invocations, 0);
        assert!(settled.quarantine_reason.is_none());
        let reports = queue.drain_reports();
        assert_eq!(reports.reports.len(), 1);
        assert_eq!(reports.reports[0].invocation_id, invocation_id);
        assert!(
            reports.reports[0]
                .result
                .as_ref()
                .expect("late result")
                .accepted
        );
        assert_eq!(queue.settlement(), settled);
    }

    #[test]
    fn worker_panic_settles_active_and_pending_event_tickets() {
        let shared = EventQueueShared::default();
        let (first_tx, first_rx) = mpsc::sync_channel(1);
        let (second_tx, second_rx) = mpsc::sync_channel(1);
        enqueue_event(&shared, queue_test_event(1), Some(first_tx)).expect("first event");
        enqueue_event(&shared, queue_test_event(2), Some(second_tx)).expect("second event");
        let active = match take_event_queue_message(&shared).expect("active event") {
            QueueMessage::Event {
                invocation_id,
                event,
                completion,
            } => {
                let event = event.0;
                ActiveEvent {
                    invocation_id,
                    run_id: event.run_id,
                    session_id: event.session_id,
                    event_name: event.event_name,
                    completion,
                }
            }
            _ => panic!("expected event"),
        };
        let reason = WasmPluginHostError::WorkerPanic("event-hook queue worker".to_owned());
        fail_event_queue_after_worker_panic(&shared, Some(active), &reason);
        let first = first_rx
            .recv_timeout(Duration::ZERO)
            .expect("active receipt");
        let second = second_rx
            .recv_timeout(Duration::ZERO)
            .expect("pending receipt");
        assert_eq!(first.invocation_id, 1);
        assert_eq!(first.result, Err(reason.clone()));
        assert_eq!(second.invocation_id, 2);
        assert_eq!(
            second.result,
            Err(WasmPluginHostError::Quarantined {
                reason: Arc::new(reason.clone())
            })
        );
        let state = shared.state.lock().unwrap_or_else(|p| p.into_inner());
        assert_eq!(state.completed_invocations, state.accepted_invocations);
        assert_eq!(state.failed_invocations, 2);
        assert_eq!(state.quarantine_reason, Some(reason));
    }

    #[test]
    fn event_flush_returns_the_worker_snapshot_not_a_later_caller_snapshot() {
        let shared = EventQueueShared::default();
        let receiver = begin_event_queue_flush(&shared).expect("barrier");
        let sender = match take_event_queue_message(&shared).expect("barrier message") {
            QueueMessage::Barrier(sender) => sender,
            _ => panic!("expected barrier"),
        };
        complete_event_queue_flush(&shared, sender);
        enqueue_event(&shared, queue_test_event(1), None).expect("later event");
        let snapshot = receiver
            .recv_timeout(Duration::ZERO)
            .expect("worker snapshot");
        assert_eq!(snapshot.accepted_invocations, 0);
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
    fn event_ids_never_wrap_and_report_storage_stays_bounded() {
        let shared = EventQueueShared::default();
        shared
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .next_event_id = u64::MAX;
        assert_eq!(
            enqueue_event(&shared, queue_test_event(1), None).expect("last id"),
            WasmPipelineEventEnqueueStatus::Enqueued {
                invocation_id: u64::MAX
            }
        );
        assert_eq!(
            enqueue_event(&shared, queue_test_event(2), None),
            Err(WasmPluginHostError::Queue(
                crate::WasmQueueError::IdExhausted
            ))
        );
        for id in 0..=WASM_PLUGIN_EVENT_REPORT_QUEUE_CAPACITY {
            shared.reports.push(WasmPipelineEventHookReport {
                invocation_id: u64::try_from(id).expect("test id"),
                run_id: "run".to_owned(),
                session_id: "session".to_owned(),
                event_name: "event".to_owned(),
                result: Ok(PipelineEventHookOutcome::default()),
                logs: Vec::new(),
            });
        }
        assert_eq!(
            shared.reports.drain().len(),
            WASM_PLUGIN_EVENT_REPORT_QUEUE_CAPACITY
        );
        assert_eq!(shared.reports.dropped_reports.load(Ordering::Relaxed), 1);
    }
}
