#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;

mod bindings {
    player_plugin_wasm::generate!({
        path: "wit",
        world: "event-and-benchmark-plugin",
        runtime_path: "player_plugin_wasm::rt",
    });
}

struct Component;

impl bindings::exports::vesper::plugin::event_hook::Guest for Component {
    fn on_event(
        _event: bindings::vesper::plugin::protocol::PipelineEvent,
    ) -> Result<
        bindings::vesper::plugin::protocol::EventHookOutcome,
        bindings::vesper::plugin::protocol::PluginError,
    > {
        Ok(bindings::vesper::plugin::protocol::EventHookOutcome {
            accepted: true,
            measurements: Vec::new(),
            diagnostics: Vec::new(),
        })
    }
}

impl bindings::exports::vesper::plugin::benchmark_sink::Guest for Component {
    fn on_event_batch(
        batch: bindings::vesper::plugin::protocol::BenchmarkBatch,
    ) -> Result<u64, bindings::vesper::plugin::protocol::PluginError> {
        Ok(u64::try_from(batch.events.len()).unwrap_or(u64::MAX))
    }

    fn flush() -> Result<
        bindings::vesper::plugin::protocol::BenchmarkReport,
        bindings::vesper::plugin::protocol::PluginError,
    > {
        Ok(bindings::vesper::plugin::protocol::BenchmarkReport {
            accepted_events: 0,
            dropped_events: 0,
            measurements: Vec::new(),
            threshold_violations: Vec::new(),
            diagnostics: Vec::new(),
        })
    }
}

player_plugin_wasm::export_component!(bindings, Component);
