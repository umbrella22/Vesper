use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use player_plugin::{
    PipelineEvent, PipelineEventHook, PipelineEventHookError, PipelineEventHookOutcome,
    PluginDiagnostic, PluginReference,
};

use crate::{PlayerError, PlayerErrorCategory, PlayerErrorCode, PlayerResult};

use super::types::{DownloadContentFormat, DownloadTaskSnapshot};

pub const MAX_PIPELINE_EVENT_HOOKS: usize = 256;
pub const MAX_PENDING_PIPELINE_EVENTS: usize = 1_024;
pub const MAX_PENDING_PIPELINE_EVENT_REPORTS: usize = 1_024;
/// Maximum drain time for synchronous teardown on a background thread.
const DEFAULT_EVENT_HOOK_CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
const WORKER_RUNNING: u8 = 0;
const WORKER_FINISHED: u8 = 1;
const WORKER_PANICKED: u8 = 2;

#[derive(Clone)]
pub struct PipelineEventHookRegistration {
    reference: PluginReference,
    hook: Arc<dyn PipelineEventHook>,
}

impl PipelineEventHookRegistration {
    pub fn new(reference: PluginReference, hook: Arc<dyn PipelineEventHook>) -> PlayerResult<Self> {
        if reference.capability_instance_id().is_none() {
            return Err(PlayerError::with_category(
                PlayerErrorCode::InvalidArgument,
                PlayerErrorCategory::Input,
                format!(
                    "event-hook registration for '{}' requires a resolved capability instance",
                    reference.plugin_id()
                ),
            ));
        }
        Ok(Self { reference, hook })
    }

    pub fn reference(&self) -> &PluginReference {
        &self.reference
    }
}

impl fmt::Debug for PipelineEventHookRegistration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PipelineEventHookRegistration")
            .field("reference", &self.reference)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PipelineEventHookReport {
    pub reference: PluginReference,
    pub run_id: String,
    pub session_id: String,
    pub event_name: String,
    pub result: Result<PipelineEventHookOutcome, PipelineEventHookError>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PipelineEventHookReportBatch {
    pub reports: Vec<PipelineEventHookReport>,
    pub dropped_events: u64,
    pub dropped_reports: u64,
    pub dispatcher_error: Option<String>,
}

#[derive(Debug)]
enum DispatchMessage {
    Event(Box<PipelineEvent>),
    Barrier(mpsc::SyncSender<()>),
}

#[derive(Debug, Default)]
struct ReportSink {
    reports: Mutex<VecDeque<PipelineEventHookReport>>,
    dropped_reports: AtomicU64,
}

impl ReportSink {
    fn push(&self, report: PipelineEventHookReport) {
        let mut reports = self
            .reports
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if reports.len() >= MAX_PENDING_PIPELINE_EVENT_REPORTS {
            self.dropped_reports.fetch_add(1, Ordering::Relaxed);
            return;
        }
        reports.push_back(report);
    }

    fn drain(&self) -> Vec<PipelineEventHookReport> {
        self.reports
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .drain(..)
            .collect()
    }
}

struct PipelineEventDispatcherInner {
    sender: Mutex<Option<SyncSender<DispatchMessage>>>,
    worker_id: Option<ThreadId>,
    worker_state: Arc<AtomicU8>,
    closed: AtomicBool,
    cancelled: Arc<AtomicBool>,
    close_failed: AtomicBool,
    report_sink: Arc<ReportSink>,
    dropped_events: Arc<AtomicU64>,
    dispatcher_error: Mutex<Option<String>>,
}

impl fmt::Debug for PipelineEventDispatcherInner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PipelineEventDispatcherInner")
            .field(
                "available",
                &self
                    .sender
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .is_some(),
            )
            .field(
                "dropped_events",
                &self.dropped_events.load(Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}

impl PipelineEventDispatcherInner {
    fn request_close(&self) {
        self.closed.store(true, Ordering::Release);
        self.sender
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
    }

    fn close(&self, timeout: Duration) -> bool {
        self.request_close();
        let start = Instant::now();
        loop {
            let state = self.worker_state.load(Ordering::Acquire);
            if state != WORKER_RUNNING {
                if state == WORKER_PANICKED {
                    let mut error = self
                        .dispatcher_error
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    if error.is_none() {
                        *error = Some("event-hook worker panicked during shutdown".to_owned());
                    }
                }
                return state == WORKER_FINISHED && !self.close_failed.load(Ordering::Acquire);
            }
            if self.worker_id == Some(std::thread::current().id())
                || self.close_failed.load(Ordering::Acquire)
                || start.elapsed() >= timeout
            {
                self.close_failed.store(true, Ordering::Release);
                self.cancelled.store(true, Ordering::Release);
                let mut error = self
                    .dispatcher_error
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if error.is_none() {
                    *error = Some("event-hook close timed out; pending events cancelled and worker quarantined".to_owned());
                }
                return false;
            }
            std::thread::sleep(
                Duration::from_millis(1).min(timeout.saturating_sub(start.elapsed())),
            );
        }
    }
}

impl Drop for PipelineEventDispatcherInner {
    fn drop(&mut self) {
        self.request_close();
        self.cancelled.store(true, Ordering::Release);
        // The detached worker owns the hooks and report sink until any in-flight
        // callback returns. Never release these owners from the calling thread.
    }
}

#[derive(Debug, Clone)]
pub struct PipelineEventDispatcher {
    inner: Arc<PipelineEventDispatcherInner>,
}

impl PipelineEventDispatcher {
    pub fn new(registrations: Vec<PipelineEventHookRegistration>) -> Self {
        let report_sink = Arc::new(ReportSink::default());
        if registrations.is_empty() {
            return Self::without_worker(report_sink, None);
        }
        if registrations.len() > MAX_PIPELINE_EVENT_HOOKS {
            return Self::without_worker(
                report_sink,
                Some(format!(
                    "event-hook registrations exceed the {MAX_PIPELINE_EVENT_HOOKS}-hook limit"
                )),
            );
        }

        let (sender, receiver) = mpsc::sync_channel(MAX_PENDING_PIPELINE_EVENTS);
        let worker_sink = report_sink.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let dropped_events = Arc::new(AtomicU64::new(0));
        let worker_dropped_events = dropped_events.clone();
        let worker_state = Arc::new(AtomicU8::new(WORKER_RUNNING));
        let completion = worker_state.clone();
        let spawn_result = std::thread::Builder::new()
            .name("vesper-pipeline-event-hook".to_owned())
            .spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    run_worker(
                        receiver,
                        registrations,
                        worker_sink,
                        worker_cancelled,
                        worker_dropped_events,
                    );
                }));
                completion.store(
                    if result.is_ok() {
                        WORKER_FINISHED
                    } else {
                        WORKER_PANICKED
                    },
                    Ordering::Release,
                );
            });

        match spawn_result {
            Ok(_worker) => Self {
                inner: Arc::new(PipelineEventDispatcherInner {
                    sender: Mutex::new(Some(sender)),
                    // Do not join even after completion: native TLS destructors
                    // can also block. Dropping the handle detaches the OS thread.
                    worker_id: Some(_worker.thread().id()),
                    worker_state,
                    closed: AtomicBool::new(false),
                    cancelled,
                    close_failed: AtomicBool::new(false),
                    report_sink,
                    dropped_events,
                    dispatcher_error: Mutex::new(None),
                }),
            },
            Err(error) => Self::without_worker(
                report_sink,
                Some(format!("failed to start event-hook worker: {error}")),
            ),
        }
    }

    fn without_worker(report_sink: Arc<ReportSink>, error: Option<String>) -> Self {
        Self {
            inner: Arc::new(PipelineEventDispatcherInner {
                sender: Mutex::new(None),
                worker_id: None,
                worker_state: Arc::new(AtomicU8::new(WORKER_FINISHED)),
                closed: AtomicBool::new(false),
                cancelled: Arc::new(AtomicBool::new(false)),
                close_failed: AtomicBool::new(false),
                report_sink,
                dropped_events: Arc::new(AtomicU64::new(0)),
                dispatcher_error: Mutex::new(error),
            }),
        }
    }

    pub fn enqueue(&self, event: PipelineEvent) {
        if self.inner.closed.load(Ordering::Acquire) {
            return;
        }
        if let Err(error) = event.validate() {
            self.record_dispatcher_error(error.to_string());
            self.inner.dropped_events.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let sender = self
            .inner
            .sender
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(sender) = sender else {
            if self
                .inner
                .dispatcher_error
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_some()
            {
                self.inner.dropped_events.fetch_add(1, Ordering::Relaxed);
            }
            return;
        };
        match sender.try_send(DispatchMessage::Event(Box::new(event))) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.inner.dropped_events.fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Disconnected(_)) => {
                self.inner.dropped_events.fetch_add(1, Ordering::Relaxed);
                self.record_dispatcher_error("event-hook worker disconnected".to_owned());
            }
        }
    }

    /// Records events intentionally omitted by a bounded host-side batch.
    ///
    /// The counter is only advanced while a worker-backed dispatcher exists;
    /// a dispatcher with no registrations has no consumer and therefore does
    /// not report dropped work.
    pub fn record_dropped_events(&self, count: u64) {
        if count == 0 {
            return;
        }
        let has_worker = self
            .inner
            .sender
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some();
        if has_worker {
            self.inner
                .dropped_events
                .fetch_add(count, Ordering::Relaxed);
        }
    }

    pub fn flush(&self, timeout: Duration) -> bool {
        let sender = self
            .inner
            .sender
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(sender) = sender else {
            return self
                .inner
                .dispatcher_error
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_none();
        };
        let (barrier_tx, barrier_rx) = mpsc::sync_channel(0);
        let deadline = Instant::now().checked_add(timeout);
        let mut message = DispatchMessage::Barrier(barrier_tx);
        loop {
            match sender.try_send(message) {
                Ok(()) => break,
                Err(TrySendError::Disconnected(_)) => return false,
                Err(TrySendError::Full(returned_message)) => {
                    let Some(deadline) = deadline else {
                        return false;
                    };
                    if Instant::now() >= deadline {
                        return false;
                    }
                    message = returned_message;
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
        let remaining = deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
            .unwrap_or(timeout);
        barrier_rx.recv_timeout(remaining).is_ok()
    }

    /// Drains accepted events on a background thread within the default deadline.
    /// Returns false if a callback prevents shutdown; see `close_with_timeout`.
    pub fn close(&self) -> bool {
        self.close_with_timeout(DEFAULT_EVENT_HOOK_CLOSE_TIMEOUT)
    }

    /// Stops submissions and waits up to `timeout` for accepted events to drain.
    /// On timeout, queued callbacks are cancelled and the running worker retains
    /// its owners until it returns. Subsequent closes also report failure.
    /// Dropping a dispatcher never waits for plugin code.
    pub fn close_with_timeout(&self, timeout: Duration) -> bool {
        self.inner.close(timeout)
    }

    pub fn drain_reports(&self) -> PipelineEventHookReportBatch {
        PipelineEventHookReportBatch {
            reports: self.inner.report_sink.drain(),
            dropped_events: self.inner.dropped_events.swap(0, Ordering::Relaxed),
            dropped_reports: self
                .inner
                .report_sink
                .dropped_reports
                .swap(0, Ordering::Relaxed),
            dispatcher_error: self
                .inner
                .dispatcher_error
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone(),
        }
    }

    fn record_dispatcher_error(&self, error: String) {
        let mut current = self
            .inner
            .dispatcher_error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if current.is_none() {
            *current = Some(error);
        }
    }
}

fn run_worker(
    receiver: mpsc::Receiver<DispatchMessage>,
    registrations: Vec<PipelineEventHookRegistration>,
    report_sink: Arc<ReportSink>,
    cancelled: Arc<AtomicBool>,
    dropped_events: Arc<AtomicU64>,
) {
    while let Ok(message) = receiver.recv() {
        match message {
            DispatchMessage::Event(event) => {
                for registration in &registrations {
                    if cancelled.load(Ordering::Acquire) {
                        dropped_events.fetch_add(1, Ordering::Relaxed);
                        break;
                    }
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        match registration.hook.on_event(&event) {
                            Ok(outcome) => {
                                outcome.validate()?;
                                Ok(outcome)
                            }
                            Err(error) => match error.validate_author_failure() {
                                Ok(()) => Err(error),
                                Err(protocol_error) => Err(protocol_error),
                            },
                        }
                    }))
                    .unwrap_or_else(|_| {
                        Err(PipelineEventHookError::Failed(
                            "event hook panicked".to_owned(),
                        ))
                    });
                    report_sink.push(PipelineEventHookReport {
                        reference: registration.reference.clone(),
                        run_id: event.run_id.clone(),
                        session_id: event.session_id.clone(),
                        event_name: event.event_name.clone(),
                        result,
                    });
                }
            }
            DispatchMessage::Barrier(acknowledge) => {
                let _ = acknowledge.send(());
            }
        }
    }
}

pub(super) fn download_pipeline_event(
    snapshot: &DownloadTaskSnapshot,
    platform: &str,
    event_name: &str,
    attributes: BTreeMap<String, String>,
    diagnostic: Option<PluginDiagnostic>,
) -> PipelineEvent {
    let resource_identity = format!("download-task:{}", snapshot.task_id.get());
    let timestamp_ns = snapshot
        .updated_at
        .saturating_duration_since(snapshot.created_at)
        .as_nanos()
        .min(u128::from(u64::MAX)) as u64;
    PipelineEvent {
        run_id: resource_identity.clone(),
        session_id: snapshot.task_id.get().to_string(),
        platform: platform.to_owned(),
        protocol: download_protocol(snapshot.source.content_format).map(str::to_owned),
        event_name: event_name.to_owned(),
        timestamp_ns,
        thread: None,
        resource_identity: Some(resource_identity),
        attributes,
        diagnostic,
    }
}

fn download_protocol(content_format: DownloadContentFormat) -> Option<&'static str> {
    match content_format {
        DownloadContentFormat::HlsSegments => Some("hls"),
        DownloadContentFormat::DashSegments => Some("dash"),
        DownloadContentFormat::FlvSegments => Some("flv"),
        DownloadContentFormat::SingleFile | DownloadContentFormat::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use super::PipelineEventDispatcher;
    use player_plugin::{
        PipelineEvent, PipelineEventHook, PipelineEventHookOutcome, PluginReference,
        PluginTransport,
    };
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::time::Duration;

    struct NoopHook;

    impl PipelineEventHook for NoopHook {
        fn on_event(
            &self,
            _event: &PipelineEvent,
        ) -> Result<PipelineEventHookOutcome, player_plugin::PipelineEventHookError> {
            Ok(PipelineEventHookOutcome::accepted())
        }
    }

    fn registration() -> super::PipelineEventHookRegistration {
        super::PipelineEventHookRegistration::new(
            PluginReference::new(
                "dev.vesper.dispatcher-test",
                Some("dev.vesper.dispatcher-test.primary".to_owned()),
                PluginTransport::Native,
            )
            .expect("valid reference"),
            Arc::new(NoopHook),
        )
        .expect("valid registration")
    }

    struct BlockingHook {
        entered: std::sync::mpsc::SyncSender<()>,
        release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
        dropped: std::sync::mpsc::SyncSender<()>,
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl PipelineEventHook for BlockingHook {
        fn on_event(
            &self,
            _event: &PipelineEvent,
        ) -> Result<PipelineEventHookOutcome, player_plugin::PipelineEventHookError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.entered.send(()).expect("entered");
            let _ = self.release.lock().expect("release lock").recv();
            Ok(PipelineEventHookOutcome::accepted())
        }
    }

    impl Drop for BlockingHook {
        fn drop(&mut self) {
            let _ = self.dropped.send(());
        }
    }

    #[test]
    fn close_deadline_and_drop_never_wait_for_blocked_hooks() {
        for explicit_close in [true, false] {
            let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let (dropped_tx, dropped_rx) = std::sync::mpsc::sync_channel(1);
            let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let mut registration = registration();
            registration.hook = Arc::new(BlockingHook {
                entered: entered_tx,
                release: std::sync::Mutex::new(release_rx),
                dropped: dropped_tx,
                calls: calls.clone(),
            });
            let dispatcher = PipelineEventDispatcher::new(vec![registration]);
            let event = PipelineEvent {
                run_id: "run".to_owned(),
                session_id: "session".to_owned(),
                platform: "test".to_owned(),
                protocol: None,
                event_name: "pipeline.test".to_owned(),
                timestamp_ns: 0,
                thread: None,
                resource_identity: None,
                attributes: BTreeMap::new(),
                diagnostic: None,
            };
            dispatcher.enqueue(event.clone());
            entered_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("hook started");
            dispatcher.enqueue(event);
            let start = std::time::Instant::now();
            if explicit_close {
                assert!(!dispatcher.close_with_timeout(Duration::from_millis(20)));
                assert!(!dispatcher.close_with_timeout(Duration::ZERO));
                assert!(
                    dispatcher
                        .drain_reports()
                        .dispatcher_error
                        .expect("timeout diagnostic")
                        .contains("quarantined")
                );
            }
            drop(dispatcher);
            assert!(start.elapsed() < Duration::from_millis(500));
            assert!(
                dropped_rx.try_recv().is_err(),
                "in-flight hook must retain its owner"
            );
            drop(release_tx);
            dropped_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("worker eventually releases its owner");
            assert_eq!(
                calls.load(std::sync::atomic::Ordering::SeqCst),
                1,
                "cancelled queue must not invoke another hook"
            );
        }
    }

    #[test]
    fn disabled_dispatcher_does_not_report_valid_events_as_dropped() {
        let dispatcher = PipelineEventDispatcher::new(Vec::new());
        dispatcher.enqueue(PipelineEvent {
            run_id: "run".to_owned(),
            session_id: "session".to_owned(),
            platform: "test".to_owned(),
            protocol: None,
            event_name: "pipeline.test".to_owned(),
            timestamp_ns: 0,
            thread: None,
            resource_identity: Some("resource".to_owned()),
            attributes: BTreeMap::new(),
            diagnostic: None,
        });

        assert!(dispatcher.flush(Duration::from_millis(1)));
        let batch = dispatcher.drain_reports();
        assert!(batch.reports.is_empty());
        assert_eq!(batch.dropped_events, 0);
        assert_eq!(batch.dropped_reports, 0);
        assert!(batch.dispatcher_error.is_none());
    }

    #[test]
    fn active_dispatcher_close_is_idempotent_and_drains_accepted_events() {
        let dispatcher = PipelineEventDispatcher::new(vec![registration()]);
        dispatcher.enqueue(PipelineEvent {
            run_id: "run".to_owned(),
            session_id: "session".to_owned(),
            platform: "test".to_owned(),
            protocol: None,
            event_name: "pipeline.test".to_owned(),
            timestamp_ns: 0,
            thread: None,
            resource_identity: Some("resource".to_owned()),
            attributes: BTreeMap::new(),
            diagnostic: None,
        });

        assert!(dispatcher.close());
        assert!(dispatcher.close());
        let batch = dispatcher.drain_reports();
        assert_eq!(batch.reports.len(), 1);
        assert!(batch.dispatcher_error.is_none());
    }
}
