use super::*;

#[test]
fn dynamic_pipeline_event_hook_adapter_round_trips_json() {
    if let Ok(mut events) = EVENTS.lock() {
        events.clear();
    }

    let api = fixture_hook_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_PLUGIN_ABI_VERSION_V2,
        plugin_kind: VesperPluginKind::PipelineEventHook,
        plugin_name: HOOK_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperPipelineEventHookApi).cast(),
    };

    let plugin = LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load hook");
    let hook = plugin
        .pipeline_event_hook()
        .expect("event hook should be available");

    hook.on_event(&PipelineEvent::DownloadTaskCompleted {
        task_id: "42".to_owned(),
    });

    let events = EVENTS
        .lock()
        .map(|events| events.clone())
        .unwrap_or_default();
    assert_eq!(
        events,
        vec![PipelineEvent::DownloadTaskCompleted {
            task_id: "42".to_owned(),
        }]
    );
}

#[test]
fn dynamic_benchmark_sink_adapter_round_trips_json() {
    if let Ok(mut batches) = BENCHMARK_BATCHES.lock() {
        batches.clear();
    }

    let api = fixture_benchmark_sink_api();
    let descriptor = VesperPluginDescriptor {
        abi_version: VESPER_PLUGIN_ABI_VERSION_V2,
        plugin_kind: VesperPluginKind::BenchmarkSink,
        plugin_name: SINK_NAME.as_ptr().cast::<c_char>(),
        api: (&api as *const VesperBenchmarkSinkApi).cast(),
    };

    let plugin =
        LoadedDynamicPlugin::from_descriptor(None, &descriptor).expect("load benchmark sink");
    assert!(plugin.post_download_processor().is_none());
    assert!(plugin.pipeline_event_hook().is_none());

    let sink = plugin
        .benchmark_sink()
        .expect("benchmark sink should be available");
    let event = BenchmarkEvent {
        run_id: "run-1".to_owned(),
        session_id: "session-1".to_owned(),
        platform: "ios".to_owned(),
        source_protocol: Some("dash".to_owned()),
        event_name: "first_frame_rendered".to_owned(),
        timestamp_ns: 100,
        elapsed_ns: 90,
        thread: Some("main".to_owned()),
        attributes: BTreeMap::from([("width".to_owned(), "1920".to_owned())]),
    };
    let status = sink
        .on_event_batch(&BenchmarkEventBatch {
            events: vec![event.clone()],
        })
        .expect("batch should be accepted");
    let report = sink.flush().expect("flush should succeed");

    assert_eq!(sink.name(), "fixture-benchmark-sink");
    assert_eq!(status.accepted_events, 1);
    assert_eq!(
        BENCHMARK_BATCHES
            .lock()
            .map(|batches| batches.clone())
            .unwrap_or_default(),
        vec![BenchmarkEventBatch {
            events: vec![event],
        }]
    );
    assert_eq!(
        report,
        BenchmarkSinkReport {
            accepted_events: 1,
            dropped_events: 0,
            plugin_errors: Vec::new(),
        }
    );
}
