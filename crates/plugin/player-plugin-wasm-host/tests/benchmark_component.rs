use std::collections::BTreeMap;
use std::time::Duration;

use player_plugin::{BenchmarkEvent, BenchmarkEventBatch, BenchmarkSink};
use player_plugin_wasm_host::{
    VESPER_PLUGIN_WIT, WasmBenchmarkBatchEnqueueStatus, WasmBenchmarkSinkAdapter,
    WasmBenchmarkSinkQueue, WasmBenchmarkSinkSession, WasmPluginHostError, WasmPluginLogLevel,
    WasmPluginLogRecord, WasmPluginRuntime,
};

// The fixed offsets model canonical ABI return areas only; product contracts come from WIT.
const FIXTURE_CORE_WAT: &str = r#"
(module
  (import "vesper:plugin/host" "log"
    (func $log (param i32 i32 i32 i32 i32)))
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 4096))
  (global $accepted (mut i64) (i64.const 0))
  (data (i32.const 1024) "fixture.batch-accepted")
  (data (i32.const 1050) "benchmark batch accepted")

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

  (func $on-event-batch (param $events-ptr i32) (param $events-len i32) (result i32)
    i32.const 2
    i32.const 1024
    i32.const 22
    i32.const 1050
    i32.const 24
    call $log
    global.get $accepted
    local.get $events-len
    i64.extend_i32_u
    i64.add
    global.set $accepted
    i32.const 0
    i32.const 0
    i32.store8
    i32.const 8
    local.get $events-len
    i64.extend_i32_u
    i64.store
    i32.const 0)
  (export "vesper:plugin/benchmark-sink#on-event-batch" (func $on-event-batch))

  (func $post-on-event-batch (param i32))
  (export "cabi_post_vesper:plugin/benchmark-sink#on-event-batch"
    (func $post-on-event-batch))

  (func $flush (result i32)
    i32.const 64
    i32.const 0
    i32.store8
    i32.const 72
    global.get $accepted
    i64.store
    i32.const 80
    i64.const 0
    i64.store
    i32.const 88
    i64.const 0
    i64.store
    i32.const 96
    i64.const 0
    i64.store
    i32.const 104
    i64.const 0
    i64.store
    i32.const 64)
  (export "vesper:plugin/benchmark-sink#flush" (func $flush))

  (func $post-flush (param i32))
  (export "cabi_post_vesper:plugin/benchmark-sink#flush" (func $post-flush)))
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

  (func $on-event-batch (param i32 i32) (result i32)
    (loop $again
      br $again)
    i32.const 0)
  (export "vesper:plugin/benchmark-sink#on-event-batch" (func $on-event-batch))
  (func $post-on-event-batch (param i32))
  (export "cabi_post_vesper:plugin/benchmark-sink#on-event-batch"
    (func $post-on-event-batch))

  (func $flush (result i32)
    i32.const 64
    i32.const 0
    i32.store8
    i32.const 72
    i64.const 0
    i64.store
    i32.const 80
    i64.const 0
    i64.store
    i32.const 88
    i64.const 0
    i64.store
    i32.const 96
    i64.const 0
    i64.store
    i32.const 104
    i64.const 0
    i64.store
    i32.const 64)
  (export "vesper:plugin/benchmark-sink#flush" (func $flush))
  (func $post-flush (param i32))
  (export "cabi_post_vesper:plugin/benchmark-sink#flush" (func $post-flush)))
"#;

#[test]
fn benchmark_component_round_trips_batch_log_and_flush() {
    let component = benchmark_fixture_component(FIXTURE_CORE_WAT);
    let runtime = WasmPluginRuntime::new().expect("WASM runtime");
    let mut session = WasmBenchmarkSinkSession::from_component_bytes(&runtime, &component)
        .expect("fixture component session");
    let batch = BenchmarkEventBatch {
        events: vec![BenchmarkEvent {
            run_id: "fixture-run".to_owned(),
            session_id: "fixture-session".to_owned(),
            platform: "test".to_owned(),
            source_protocol: Some("fixture".to_owned()),
            event_name: "fixture.completed".to_owned(),
            timestamp_ns: 10,
            elapsed_ns: 5,
            thread: Some("test-thread".to_owned()),
            attributes: BTreeMap::from([("codec".to_owned(), "fixture".to_owned())]),
        }],
    };

    let status = session.on_event_batch(&batch).expect("accepted batch");
    assert_eq!(status.accepted_events, 1);
    assert_eq!(
        session.take_logs(),
        vec![WasmPluginLogRecord {
            level: WasmPluginLogLevel::Info,
            code: "fixture.batch-accepted".to_owned(),
            message: "benchmark batch accepted".to_owned(),
        }]
    );

    let report = session.flush().expect("fixture report");
    assert_eq!(report.accepted_events, 1);
    assert_eq!(report.dropped_events, 0);
    assert!(report.measurements.is_empty());
    assert!(report.threshold_violations.is_empty());
    assert!(report.diagnostics.is_empty());
    assert!(!session.is_quarantined());
}

#[test]
fn benchmark_adapter_exposes_the_transport_neutral_capability() {
    let component = benchmark_fixture_component(FIXTURE_CORE_WAT);
    let runtime = WasmPluginRuntime::new().expect("WASM runtime");
    let adapter =
        WasmBenchmarkSinkAdapter::from_component_bytes("fixture-benchmark", &runtime, &component)
            .expect("fixture component adapter");

    assert_eq!(adapter.name(), "fixture-benchmark");
    assert_eq!(
        adapter
            .on_event_batch(&benchmark_batch(10))
            .expect("accepted batch")
            .accepted_events,
        1
    );
    assert_eq!(adapter.flush().expect("fixture report").accepted_events, 1);
}

#[test]
fn benchmark_queue_preserves_fifo_reports_and_idempotent_close() {
    let component = benchmark_fixture_component(FIXTURE_CORE_WAT);
    let runtime = WasmPluginRuntime::new().expect("WASM runtime");
    let queue = WasmBenchmarkSinkQueue::from_component_bytes(&runtime, &component)
        .expect("fixture component queue");

    assert_eq!(
        queue.enqueue(benchmark_batch(10)).expect("first enqueue"),
        WasmBenchmarkBatchEnqueueStatus::Enqueued { batch_id: 1 }
    );
    assert_eq!(
        queue.enqueue(benchmark_batch(20)).expect("second enqueue"),
        WasmBenchmarkBatchEnqueueStatus::Enqueued { batch_id: 2 }
    );

    let first_flush = queue.flush(Duration::from_secs(1)).expect("first flush");
    assert_eq!(first_flush.result.expect("first report").accepted_events, 2);
    let second_flush = queue.flush(Duration::from_secs(1)).expect("second flush");
    assert_eq!(
        second_flush.result.expect("second report").accepted_events,
        2
    );

    let reports = queue.drain_reports();
    assert_eq!(reports.queue_dropped_batches, 0);
    assert_eq!(reports.queue_dropped_events, 0);
    assert_eq!(reports.dropped_reports, 0);
    assert_eq!(reports.reports.len(), 2);
    assert_eq!(reports.reports[0].batch_id, 1);
    assert_eq!(reports.reports[1].batch_id, 2);
    assert_eq!(reports.reports[0].event_count, 1);
    assert_eq!(reports.reports[1].event_count, 1);
    for report in reports.reports {
        assert_eq!(report.result.expect("batch report").accepted_events, 1);
        assert_eq!(
            report.logs,
            vec![WasmPluginLogRecord {
                level: WasmPluginLogLevel::Info,
                code: "fixture.batch-accepted".to_owned(),
                message: "benchmark batch accepted".to_owned(),
            }]
        );
    }

    let close = queue.close(Duration::from_secs(1)).expect("close");
    assert_eq!(
        close.result.as_ref().expect("close report").accepted_events,
        2
    );
    assert_eq!(
        queue.close(Duration::ZERO).expect("idempotent close"),
        close
    );
    assert!(matches!(
        queue.enqueue(benchmark_batch(30)),
        Err(WasmPluginHostError::Queue(
            player_plugin_wasm_host::WasmQueueError::Closed
        ))
    ));
}

#[test]
fn benchmark_queue_quarantines_after_execution_timeout() {
    let component = benchmark_fixture_component(TRAPPING_CORE_WAT);
    let runtime = WasmPluginRuntime::new().expect("WASM runtime");
    let queue = WasmBenchmarkSinkQueue::from_component_bytes(&runtime, &component)
        .expect("trapping component queue");

    assert_eq!(
        queue
            .enqueue(benchmark_batch(10))
            .expect("trapping enqueue"),
        WasmBenchmarkBatchEnqueueStatus::Enqueued { batch_id: 1 }
    );
    assert_eq!(
        queue
            .enqueue(benchmark_batch(20))
            .expect("quarantined enqueue"),
        WasmBenchmarkBatchEnqueueStatus::Enqueued { batch_id: 2 }
    );

    let flush = queue
        .flush(Duration::from_secs(2))
        .expect("quarantine flush");
    assert!(matches!(
        flush.result,
        Err(WasmPluginHostError::Quarantined { .. })
    ));
    let reports = queue.drain_reports();
    assert_eq!(reports.reports.len(), 2);
    assert_eq!(reports.reports[0].batch_id, 1);
    assert!(matches!(
        reports.reports[0].result,
        Err(WasmPluginHostError::Execution(_))
    ));
    assert_eq!(reports.reports[1].batch_id, 2);
    assert!(matches!(
        reports.reports[1].result,
        Err(WasmPluginHostError::Quarantined { .. })
    ));

    let close = queue
        .close(Duration::from_secs(2))
        .expect("quarantined close");
    assert!(matches!(
        close.result,
        Err(WasmPluginHostError::Quarantined { .. })
    ));
}

#[test]
fn benchmark_tickets_match_direct_execution_and_complete_after_receipt_drop() {
    let component = benchmark_fixture_component(FIXTURE_CORE_WAT);
    let runtime = WasmPluginRuntime::new().expect("WASM runtime");
    let mut direct = WasmBenchmarkSinkSession::from_component_bytes(&runtime, &component)
        .expect("direct session");
    let expected = direct
        .on_event_batch(&benchmark_batch(10))
        .expect("direct status");
    let expected_logs = direct.take_logs();
    let queue =
        WasmBenchmarkSinkQueue::from_component_bytes(&runtime, &component).expect("checked queue");
    let first = queue.submit(benchmark_batch(10)).expect("first receipt");
    assert_eq!(first.invocation_id(), 1);
    let report = first.wait(Duration::from_secs(2)).expect("first result");
    assert_eq!(report.batch_id, 1);
    assert_eq!(report.result, Ok(expected.clone()));
    assert_eq!(report.logs, expected_logs);
    let second = queue.submit(benchmark_batch(20)).expect("dropped receipt");
    assert_eq!(second.invocation_id(), 2);
    drop(second);
    let barrier = queue.flush(Duration::from_secs(2)).expect("flush barrier");
    assert_eq!(barrier.result.expect("guest flush").accepted_events, 2);
    assert_eq!(barrier.settlement.accepted_invocations, 2);
    assert_eq!(barrier.settlement.completed_invocations, 2);
    assert_eq!(barrier.settlement.pending_invocations, 0);
    assert_eq!(barrier.settlement.wait_expirations, 0);
    let reports = queue.drain_reports();
    assert_eq!(reports.reports.len(), 2);
    assert_eq!(reports.reports[1].batch_id, 2);
    assert_eq!(reports.reports[1].result, Ok(expected));
    assert_eq!(reports.reports[1].logs, expected_logs);
    let close = queue.close(Duration::from_secs(2)).expect("joined close");
    assert!(close.settlement.worker_joined);
    assert_eq!(close.settlement.failed_invocations, 0);
}

#[test]
fn benchmark_admission_accepts_empty_and_512_events_but_rejects_513_without_an_id() {
    // This fixed allocator needs enough fixture memory for the largest valid input.
    let wat = FIXTURE_CORE_WAT.replace(
        "(memory (export \"memory\") 1)",
        "(memory (export \"memory\") 8)",
    );
    let component = benchmark_fixture_component(&wat);
    let runtime = WasmPluginRuntime::new().expect("WASM runtime");
    let queue =
        WasmBenchmarkSinkQueue::from_component_bytes(&runtime, &component).expect("checked queue");
    let too_many = BenchmarkEventBatch {
        events: vec![benchmark_batch(10).events.remove(0); 513],
    };
    assert!(matches!(
        queue.submit(too_many),
        Err(WasmPluginHostError::LocalValidation(_))
    ));
    assert_eq!(queue.settlement().accepted_invocations, 0);
    assert!(queue.drain_reports().reports.is_empty());
    let empty = queue
        .submit(BenchmarkEventBatch::default())
        .expect("empty receipt");
    assert_eq!(empty.invocation_id(), 1);
    assert_eq!(
        empty
            .wait(Duration::from_secs(2))
            .expect("empty result")
            .result
            .expect("empty status")
            .accepted_events,
        0
    );
    let full = queue
        .submit(BenchmarkEventBatch {
            events: vec![benchmark_batch(10).events.remove(0); 512],
        })
        .expect("full receipt");
    assert_eq!(full.invocation_id(), 2);
    assert_eq!(
        full.wait(Duration::from_secs(2))
            .expect("full result")
            .result
            .expect("full status")
            .accepted_events,
        512
    );
    let close = queue.close(Duration::from_secs(2)).expect("close");
    assert_eq!(close.result.expect("flush report").accepted_events, 512);
    assert_eq!(close.settlement.completed_invocations, 2);
    assert_eq!(close.settlement.failed_invocations, 0);
    assert!(close.settlement.worker_joined);
}

#[test]
fn benchmark_acknowledgement_is_bound_to_the_actual_batch_count() {
    let runtime = WasmPluginRuntime::new().expect("WASM runtime");
    let anchor = "    i32.const 8\n    local.get $events-len\n    i64.extend_i32_u\n    i64.store";
    assert_eq!(FIXTURE_CORE_WAT.matches(anchor).count(), 1);
    for (accepted, valid) in [(0, true), (2, false)] {
        let wat = FIXTURE_CORE_WAT.replace(
            anchor,
            &format!("    i32.const 8\n    i64.const {accepted}\n    i64.store"),
        );
        let component = benchmark_fixture_component(&wat);
        let queue = WasmBenchmarkSinkQueue::from_component_bytes(&runtime, &component)
            .expect("checked queue");
        let report = queue
            .submit_and_wait(benchmark_batch(10), Duration::from_secs(2))
            .expect("completed receipt");
        if valid {
            assert_eq!(
                report
                    .result
                    .expect("partial acknowledgement")
                    .accepted_events,
                accepted
            );
        } else {
            let failure = report.result.expect_err("invalid acknowledgement");
            assert!(matches!(
                &failure,
                WasmPluginHostError::ProtocolViolation(_)
            ));
            assert_eq!(queue.settlement().quarantine_reason, Some(failure));
        }
        let close = queue.close(Duration::from_secs(2)).expect("joined close");
        assert!(close.settlement.worker_joined);
        assert_eq!(close.settlement.failed_invocations, u64::from(!valid));
    }
}

#[test]
fn benchmark_flush_trap_retains_cause_without_counting_a_failed_batch() {
    let component = benchmark_fixture_component(&FIXTURE_CORE_WAT.replace(
        "  (func $flush (result i32)",
        "  (func $flush (result i32)\n    unreachable",
    ));
    let runtime = WasmPluginRuntime::new().expect("WASM runtime");
    let queue =
        WasmBenchmarkSinkQueue::from_component_bytes(&runtime, &component).expect("checked queue");
    assert!(
        queue
            .submit_and_wait(benchmark_batch(10), Duration::from_secs(2))
            .expect("completed receipt")
            .result
            .is_ok()
    );
    let flush = queue.flush(Duration::from_secs(2)).expect("flush receipt");
    let failure = flush.result.expect_err("guest flush trap");
    assert!(matches!(&failure, WasmPluginHostError::Execution(cause)
        if cause.kind == player_plugin_wasm_host::WasmExecutionFailureKind::GuestTrap
        && cause.cause.error().downcast_ref::<wasmtime::Trap>() == Some(&wasmtime::Trap::UnreachableCodeReached)));
    assert_eq!(flush.settlement.quarantine_reason, Some(failure.clone()));
    assert_eq!(flush.settlement.failed_invocations, 0);
    let close = queue.close(Duration::from_secs(2)).expect("joined close");
    assert!(
        matches!(close.result, Err(WasmPluginHostError::Quarantined { reason }) if reason.as_ref() == &failure)
    );
    assert_eq!(close.settlement.quarantine_reason, Some(failure));
    assert_eq!(close.settlement.completed_invocations, 1);
    assert_eq!(close.settlement.failed_invocations, 0);
    assert!(close.settlement.worker_joined);
}

fn benchmark_batch(timestamp_ns: u64) -> BenchmarkEventBatch {
    BenchmarkEventBatch {
        events: vec![BenchmarkEvent {
            run_id: "fixture-run".to_owned(),
            session_id: "fixture-session".to_owned(),
            platform: "test".to_owned(),
            source_protocol: Some("fixture".to_owned()),
            event_name: "fixture.completed".to_owned(),
            timestamp_ns,
            elapsed_ns: timestamp_ns,
            thread: Some("test-thread".to_owned()),
            attributes: BTreeMap::from([("codec".to_owned(), "fixture".to_owned())]),
        }],
    }
}

fn benchmark_fixture_component(core_wat: &str) -> Vec<u8> {
    let mut resolve = wit_parser::Resolve::default();
    let package = resolve
        .push_str("vesper-plugin.wit", VESPER_PLUGIN_WIT)
        .expect("fixture WIT package");
    let world = resolve
        .select_world(&[package], Some("benchmark-sink-plugin"))
        .expect("benchmark fixture world");
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
