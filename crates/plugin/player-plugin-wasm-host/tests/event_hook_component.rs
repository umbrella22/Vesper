use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use player_plugin::{PipelineEvent, PipelineEventHook, PluginDiagnostic, PluginDiagnosticSeverity};
use player_plugin_wasm_host::{
    VESPER_PLUGIN_WIT, WasmPipelineEventEnqueueStatus, WasmPipelineEventHookAdapter,
    WasmPipelineEventHookQueue, WasmPipelineEventHookSession, WasmPluginHostError,
    WasmPluginLogLevel, WasmPluginLogRecord, WasmPluginRuntime,
};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;

// Fixed offsets model canonical ABI records only; product contracts come from WIT.
const SUCCESS_CORE_WAT: &str = r#"
(module
  (import "vesper:plugin/host" "log"
    (func $log (param i32 i32 i32 i32 i32)))
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 4096))
  (data (i32.const 1024) "fixture.accepted")
  (data (i32.const 1050) "event accepted")
  (data (i32.const 1080) "fixture.event")
  (data (i32.const 1100) "fixture.diagnostic")

  (func $realloc
    (param $old-ptr i32) (param $old-size i32) (param $align i32) (param $new-size i32)
    (result i32)
    (local $result i32)
    local.get $new-size
    i32.eqz
    if
      i32.const 0
      return
    end
    global.get $heap
    local.get $align
    i32.const 1
    i32.sub
    i32.add
    i32.const 0
    local.get $align
    i32.sub
    i32.and
    local.tee $result
    local.get $new-size
    i32.add
    global.set $heap
    local.get $result)
  (export "cabi_realloc" (func $realloc))

  (func $on-event (param $event i32) (result i32)
    i32.const 2
    i32.const 1080
    i32.const 13
    local.get $event
    i32.load offset=36
    local.get $event
    i32.load offset=40
    call $log

    local.get $event
    i32.load8_u offset=88
    if
      i32.const 2
      i32.const 1100
      i32.const 18
      local.get $event
      i32.load offset=92
      local.get $event
      i32.load offset=96
      call $log
    end

    i32.const 0
    i32.const 0
    i32.store8
    i32.const 4
    i32.const 1
    i32.store8
    i32.const 8
    i32.const 0
    i32.store
    i32.const 12
    i32.const 0
    i32.store
    i32.const 16
    i32.const 512
    i32.store
    i32.const 20
    i32.const 1
    i32.store

    i32.const 512
    i32.const 1024
    i32.store
    i32.const 516
    i32.const 16
    i32.store
    i32.const 520
    i32.const 0
    i32.store8
    i32.const 524
    i32.const 1050
    i32.store
    i32.const 528
    i32.const 14
    i32.store
    i32.const 532
    i32.const 0
    i32.store
    i32.const 536
    i32.const 0
    i32.store
    i32.const 0)
  (export "vesper:plugin/event-hook#on-event" (func $on-event))

  (func $post-on-event (param i32))
  (export "cabi_post_vesper:plugin/event-hook#on-event" (func $post-on-event)))
"#;

const TRAPPING_CORE_WAT: &str = r#"
(module
  (import "vesper:plugin/host" "log"
    (func $log (param i32 i32 i32 i32 i32)))
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 4096))
  (func $realloc
    (param $old-ptr i32) (param $old-size i32) (param $align i32) (param $new-size i32)
    (result i32)
    global.get $heap
    local.get $new-size
    i32.add
    global.set $heap
    global.get $heap
    local.get $new-size
    i32.sub)
  (export "cabi_realloc" (func $realloc))
  (func $on-event (param i32) (result i32)
    (loop $again
      br $again)
    i32.const 0)
  (export "vesper:plugin/event-hook#on-event" (func $on-event))
  (func $post-on-event (param i32))
  (export "cabi_post_vesper:plugin/event-hook#on-event" (func $post-on-event)))
"#;

#[test]
fn event_hook_component_round_trips_full_envelope_outcome_and_logs() {
    let component = event_fixture_component(SUCCESS_CORE_WAT);
    let runtime = WasmPluginRuntime::new().expect("WASM runtime");
    let mut session = WasmPipelineEventHookSession::from_component_bytes(&runtime, &component)
        .expect("fixture component session");

    let outcome = session.on_event(&pipeline_event()).expect("accepted event");
    assert!(outcome.accepted);
    assert!(outcome.measurements.is_empty());
    assert_eq!(outcome.diagnostics.len(), 1);
    assert_eq!(outcome.diagnostics[0].code, "fixture.accepted");
    assert_eq!(outcome.diagnostics[0].message, "event accepted");
    assert_eq!(
        session.take_logs(),
        vec![
            WasmPluginLogRecord {
                level: WasmPluginLogLevel::Info,
                code: "fixture.event".to_owned(),
                message: "vendor.future.event".to_owned(),
            },
            WasmPluginLogRecord {
                level: WasmPluginLogLevel::Info,
                code: "fixture.diagnostic".to_owned(),
                message: "download.failed".to_owned(),
            },
        ]
    );
    assert!(!session.is_quarantined());
}

#[test]
fn event_hook_adapter_exposes_the_transport_neutral_capability() {
    let component = event_fixture_component(SUCCESS_CORE_WAT);
    let runtime = WasmPluginRuntime::new().expect("WASM runtime");
    let adapter = WasmPipelineEventHookAdapter::from_component_bytes(&runtime, &component)
        .expect("fixture component adapter");

    let outcome = adapter.on_event(&pipeline_event()).expect("accepted event");
    assert!(outcome.accepted);
    assert_eq!(outcome.diagnostics[0].code, "fixture.accepted");
}

#[test]
fn event_hook_adapter_emits_logs_after_releasing_its_session_lock() {
    let component = event_fixture_component(SUCCESS_CORE_WAT);
    let runtime = WasmPluginRuntime::new().expect("WASM runtime");
    let adapter = Arc::new(
        WasmPipelineEventHookAdapter::from_component_bytes(&runtime, &component)
            .expect("fixture component adapter"),
    );
    let subscriber = tracing_subscriber::registry().with(ReentrantAdapterLayer {
        adapter: adapter.clone(),
        reentered: Arc::new(AtomicBool::new(false)),
    });

    let outcome =
        tracing::subscriber::with_default(subscriber, || adapter.on_event(&pipeline_event()))
            .expect("outer adapter call");
    assert!(outcome.accepted);
}

struct ReentrantAdapterLayer {
    adapter: Arc<WasmPipelineEventHookAdapter>,
    reentered: Arc<AtomicBool>,
}

impl<S> tracing_subscriber::Layer<S> for ReentrantAdapterLayer
where
    S: Subscriber,
{
    fn on_event(&self, _event: &Event<'_>, _context: Context<'_, S>) {
        if self.reentered.swap(true, Ordering::AcqRel) {
            return;
        }

        let adapter = self.adapter.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = adapter.on_event(&pipeline_event());
            let _ = sender.send(result);
        });
        let nested = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("tracing subscriber re-entry must not wait on the adapter session lock");
        assert!(nested.expect("nested adapter call").accepted);
    }
}

#[test]
fn event_hook_queue_reports_success_and_quarantines_a_trapping_guest() {
    let runtime = WasmPluginRuntime::new().expect("WASM runtime");
    let success_component = event_fixture_component(SUCCESS_CORE_WAT);
    let success_queue =
        WasmPipelineEventHookQueue::from_component_bytes(&runtime, &success_component)
            .expect("success queue");
    assert_eq!(
        success_queue.enqueue(pipeline_event()).expect("enqueue"),
        WasmPipelineEventEnqueueStatus::Enqueued { invocation_id: 1 }
    );
    success_queue
        .flush(Duration::from_secs(1))
        .expect("success flush");
    let success = success_queue.drain_reports();
    assert_eq!(success.reports.len(), 1);
    assert!(success.reports[0].result.is_ok());
    assert_eq!(success.dropped_events, 0);
    success_queue
        .close(Duration::from_secs(1))
        .expect("success close");
    success_queue
        .close(Duration::ZERO)
        .expect("idempotent success close");
    assert!(matches!(
        success_queue.enqueue(pipeline_event()),
        Err(WasmPluginHostError::Queue(
            player_plugin_wasm_host::WasmQueueError::Closed
        ))
    ));

    let trapping_component = event_fixture_component(TRAPPING_CORE_WAT);
    let trapping_queue =
        WasmPipelineEventHookQueue::from_component_bytes(&runtime, &trapping_component)
            .expect("trapping queue");
    assert_eq!(
        trapping_queue
            .enqueue(pipeline_event())
            .expect("enqueue trap"),
        WasmPipelineEventEnqueueStatus::Enqueued { invocation_id: 1 }
    );
    trapping_queue
        .flush(Duration::from_secs(1))
        .expect("trapping flush");
    let trapped = trapping_queue.drain_reports();
    assert_eq!(trapped.reports.len(), 1);
    assert!(matches!(
        trapped.reports[0].result,
        Err(WasmPluginHostError::Execution(_))
    ));

    assert_eq!(
        trapping_queue
            .enqueue(pipeline_event())
            .expect("enqueue quarantined event"),
        WasmPipelineEventEnqueueStatus::Enqueued { invocation_id: 2 }
    );
    trapping_queue
        .flush(Duration::from_secs(1))
        .expect("quarantine flush");
    let quarantined = trapping_queue.drain_reports();
    assert_eq!(quarantined.reports.len(), 1);
    assert!(matches!(
        quarantined.reports[0].result,
        Err(WasmPluginHostError::Quarantined { .. })
    ));
    trapping_queue
        .close(Duration::from_secs(1))
        .expect("trapping close");
}

#[test]
fn event_tickets_preserve_direct_result_and_logs_and_settle_dropped_receipts() {
    let component = event_fixture_component(SUCCESS_CORE_WAT);
    let runtime = WasmPluginRuntime::new().expect("WASM runtime");
    let mut direct = WasmPipelineEventHookSession::from_component_bytes(&runtime, &component)
        .expect("direct session");
    let expected = direct.on_event(&pipeline_event()).expect("direct outcome");
    let expected_logs = direct.take_logs();
    let queue = WasmPipelineEventHookQueue::from_component_bytes(&runtime, &component)
        .expect("checked queue");
    let first = queue.submit(pipeline_event()).expect("accepted receipt");
    assert_eq!(first.invocation_id(), 1);
    let report = first
        .wait(Duration::from_secs(2))
        .expect("completed receipt");
    assert_eq!(report.invocation_id, 1);
    assert_eq!(report.result, Ok(expected.clone()));
    assert_eq!(report.logs, expected_logs);
    let second = queue.submit(pipeline_event()).expect("dropped receipt");
    assert_eq!(second.invocation_id(), 2);
    drop(second);
    let barrier = queue
        .flush(Duration::from_secs(2))
        .expect("settled barrier");
    assert_eq!(barrier.accepted_invocations, 2);
    assert_eq!(barrier.completed_invocations, 2);
    assert_eq!(barrier.pending_invocations, 0);
    assert_eq!(barrier.failed_invocations, 0);
    assert_eq!(barrier.wait_expirations, 0);
    let reports = queue.drain_reports();
    assert_eq!(reports.reports.len(), 2);
    assert_eq!(reports.reports[1].invocation_id, 2);
    assert_eq!(reports.reports[1].result, Ok(expected));
    assert_eq!(reports.reports[1].logs, expected_logs);
    let closed = queue.close(Duration::from_secs(2)).expect("joined close");
    assert_eq!(
        closed.lifecycle,
        player_plugin_wasm_host::WasmQueueLifecycle::Closed
    );
    assert!(closed.worker_joined);
    assert_eq!(
        closed,
        queue.close(Duration::ZERO).expect("idempotent close")
    );
}

#[test]
fn event_admission_rejects_invalid_input_before_guest_execution_or_id_allocation() {
    let component = event_fixture_component(SUCCESS_CORE_WAT);
    let runtime = WasmPluginRuntime::new().expect("WASM runtime");
    let queue = WasmPipelineEventHookQueue::from_component_bytes(&runtime, &component)
        .expect("checked queue");
    let mut invalid = pipeline_event();
    invalid.run_id.clear();
    assert!(matches!(
        queue.submit(invalid),
        Err(WasmPluginHostError::LocalValidation(_))
    ));
    assert_eq!(queue.settlement().accepted_invocations, 0);
    assert!(queue.drain_reports().reports.is_empty());
    let ticket = queue.submit(pipeline_event()).expect("valid receipt");
    assert_eq!(ticket.invocation_id(), 1);
    let report = ticket.wait(Duration::from_secs(2)).expect("valid result");
    assert!(report.result.is_ok());
    assert_eq!(report.logs.len(), 2);
    assert!(
        queue
            .close(Duration::from_secs(2))
            .expect("close")
            .worker_joined
    );
}

#[test]
fn non_finite_guest_measurement_preserves_protocol_quarantine_cause() {
    let component = event_component_with_return_stores(
        r#"
    i32.const 8 i32.const 640 i32.store
    i32.const 12 i32.const 1 i32.store
    i32.const 640 i32.const 1024 i32.store
    i32.const 644 i32.const 16 i32.store
    i32.const 648 f64.const nan f64.store
    i32.const 656 i32.const 1050 i32.store
    i32.const 660 i32.const 14 i32.store
    "#,
    );
    assert_protocol_quarantine(&component, "finite");
}

#[test]
fn duplicate_guest_attributes_are_not_silently_overwritten() {
    let component = event_component_with_return_stores(
        r#"
    i32.const 532 i32.const 704 i32.store
    i32.const 536 i32.const 2 i32.store
    i32.const 704 i32.const 1024 i32.store
    i32.const 708 i32.const 16 i32.store
    i32.const 712 i32.const 1050 i32.store
    i32.const 716 i32.const 14 i32.store
    i32.const 720 i32.const 1024 i32.store
    i32.const 724 i32.const 16 i32.store
    i32.const 728 i32.const 1050 i32.store
    i32.const 732 i32.const 14 i32.store
    "#,
    );
    assert_protocol_quarantine(&component, "duplicate");
}

fn event_component_with_return_stores(stores: &str) -> Vec<u8> {
    let anchor = "    i32.const 0)\n  (export \"vesper:plugin/event-hook#on-event\"";
    assert_eq!(SUCCESS_CORE_WAT.matches(anchor).count(), 1);
    event_fixture_component(&SUCCESS_CORE_WAT.replace(anchor, &format!("{stores}\n{anchor}")))
}

fn assert_protocol_quarantine(component: &[u8], diagnostic: &str) {
    let runtime = WasmPluginRuntime::new().expect("WASM runtime");
    let mut direct = WasmPipelineEventHookSession::from_component_bytes(&runtime, component)
        .expect("direct session");
    let failure = direct
        .on_event(&pipeline_event())
        .expect_err("invalid guest outcome");
    assert!(
        matches!(&failure, WasmPluginHostError::ProtocolViolation(message) if message.contains(diagnostic))
    );
    assert_eq!(direct.quarantine_reason(), Some(&failure));
    assert!(
        matches!(direct.on_event(&pipeline_event()), Err(WasmPluginHostError::Quarantined { reason }) if reason.as_ref() == &failure)
    );
    let queue = WasmPipelineEventHookQueue::from_component_bytes(&runtime, component)
        .expect("checked queue");
    let report = queue
        .submit_and_wait(pipeline_event(), Duration::from_secs(2))
        .expect("completed receipt");
    assert_eq!(report.result, Err(failure.clone()));
    assert_eq!(report.logs, direct.take_logs());
    let settlement = queue.close(Duration::from_secs(2)).expect("joined close");
    assert_eq!(settlement.quarantine_reason, Some(failure));
    assert_eq!(settlement.failed_invocations, 1);
    assert_eq!(settlement.completed_invocations, 1);
    assert!(settlement.worker_joined);
}

fn pipeline_event() -> PipelineEvent {
    PipelineEvent {
        run_id: "fixture-run".to_owned(),
        session_id: "fixture-session".to_owned(),
        platform: "test".to_owned(),
        protocol: Some("hls".to_owned()),
        event_name: "vendor.future.event".to_owned(),
        timestamp_ns: 10,
        thread: Some("test-thread".to_owned()),
        resource_identity: Some("download-task:1".to_owned()),
        attributes: BTreeMap::from([("state".to_owned(), "failed".to_owned())]),
        diagnostic: Some(PluginDiagnostic {
            code: "download.failed".to_owned(),
            severity: PluginDiagnosticSeverity::Error,
            message: "download task failed".to_owned(),
            attributes: BTreeMap::from([("retriable".to_owned(), "false".to_owned())]),
        }),
    }
}

fn event_fixture_component(core_wat: &str) -> Vec<u8> {
    let mut resolve = wit_parser::Resolve::default();
    let package = resolve
        .push_str("vesper-plugin.wit", VESPER_PLUGIN_WIT)
        .expect("fixture WIT package");
    let world = resolve
        .select_world(&[package], Some("event-hook-plugin"))
        .expect("event-hook fixture world");
    let mut module = wat::parse_str(core_wat).expect("fixture core module");
    // Match ComponentEncoder's default full interface names.
    let canonical_names = false;
    wit_component::embed_component_metadata(
        &mut module,
        &resolve,
        world,
        wit_component::StringEncoding::UTF8,
        canonical_names,
    )
    .expect("embedded component metadata");
    wit_component::ComponentEncoder::default()
        .validate(true)
        .module(&module)
        .expect("fixture component module")
        .encode()
        .expect("encoded fixture component")
}
