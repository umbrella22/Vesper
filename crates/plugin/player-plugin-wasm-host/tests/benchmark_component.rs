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
        Err(WasmPluginHostError::Queue(message)) if message.contains("not open")
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
        Err(WasmPluginHostError::Quarantined)
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
        Err(WasmPluginHostError::Quarantined)
    ));

    let close = queue
        .close(Duration::from_secs(2))
        .expect("quarantined close");
    assert!(matches!(
        close.result,
        Err(WasmPluginHostError::Quarantined)
    ));
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
    wit_component::embed_component_metadata(
        &mut module,
        &resolve,
        world,
        wit_component::StringEncoding::UTF8,
    )
    .expect("embedded component metadata");
    wit_component::ComponentEncoder::default()
        .validate(true)
        .module(&module)
        .expect("fixture component module")
        .encode()
        .expect("encoded fixture component")
}
